mod collect;
#[cfg(target_os = "linux")]
mod linux;
mod portable;
mod reader;
mod walk;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::bounded;
use parking_lot::{Mutex, RwLock};

pub use reader::{DirBatch, RawEntry};

use crate::exclude::Excludes;
use crate::tree::{Metric, NodeId, Totals, Tree};

pub type SharedTree = Arc<RwLock<Tree>>;

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub cross_filesystems: bool,
    pub threads: usize,
    pub excludes: Excludes,
    pub dedup_hardlinks: bool,
    pub metric: Metric,
}

impl ScanOptions {
    pub fn new(root: impl Into<PathBuf>) -> ScanOptions {
        ScanOptions {
            root: root.into(),
            cross_filesystems: false,
            threads: 0,
            excludes: Excludes::default(),
            dedup_hardlinks: true,
            metric: Metric::Allocated,
        }
    }

    pub fn thread_count(&self) -> usize {
        if self.threads > 0 {
            return self.threads;
        }
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        cores.clamp(4, 16)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanState {
    Scanning,
    Finalizing,
    Done,
    Cancelled,
    Failed,
}

impl ScanState {
    fn from_u8(value: u8) -> ScanState {
        match value {
            0 => ScanState::Scanning,
            1 => ScanState::Finalizing,
            2 => ScanState::Done,
            3 => ScanState::Cancelled,
            _ => ScanState::Failed,
        }
    }
}

pub struct ScanProgress {
    pub target: NodeId,
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub alloc: AtomicU64,
    pub errors: AtomicU64,
    state: AtomicU8,
    cancel: AtomicBool,
    finished_ms: AtomicU64,
    started: Instant,
    current: Mutex<PathBuf>,
    failure: Mutex<Option<String>>,
}

impl ScanProgress {
    pub fn new(target: NodeId) -> ScanProgress {
        ScanProgress {
            target,
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            alloc: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            state: AtomicU8::new(0),
            cancel: AtomicBool::new(false),
            finished_ms: AtomicU64::new(0),
            started: Instant::now(),
            current: Mutex::new(PathBuf::new()),
            failure: Mutex::new(None),
        }
    }

    pub fn state(&self) -> ScanState {
        ScanState::from_u8(self.state.load(Ordering::Acquire))
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state(), ScanState::Scanning | ScanState::Finalizing)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn elapsed(&self) -> Duration {
        match self.finished_ms.load(Ordering::Acquire) {
            0 => self.started.elapsed(),
            ms => Duration::from_millis(ms),
        }
    }

    pub fn current(&self) -> PathBuf {
        self.current.lock().clone()
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.lock().clone()
    }

    pub fn entries(&self) -> u64 {
        self.files.load(Ordering::Relaxed) + self.dirs.load(Ordering::Relaxed)
    }

    fn set_state(&self, state: ScanState) {
        let value = match state {
            ScanState::Scanning => 0,
            ScanState::Finalizing => 1,
            ScanState::Done => 2,
            ScanState::Cancelled => 3,
            ScanState::Failed => 4,
        };
        if matches!(state, ScanState::Done | ScanState::Cancelled | ScanState::Failed) {
            let ms = self.started.elapsed().as_millis().max(1) as u64;
            self.finished_ms.store(ms, Ordering::Release);
        }
        self.state.store(value, Ordering::Release);
    }

    fn fail(&self, message: String) {
        *self.failure.lock() = Some(message);
        self.set_state(ScanState::Failed);
    }
}

pub fn root_info(path: &Path) -> std::io::Result<(Totals, u64)> {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("DIRSTAT_PORTABLE_READER").is_none() {
            use rustix::fs::{AtFlags, CWD, StatxFlags, statx};
            let stat = statx(
                CWD,
                path,
                AtFlags::empty(),
                StatxFlags::TYPE | StatxFlags::SIZE | StatxFlags::BLOCKS | StatxFlags::MTIME | StatxFlags::MNT_ID,
            )?;
            if stat.stx_mode as u32 & 0o170000 != 0o040000 {
                return Err(std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a folder"));
            }
            let totals = Totals {
                size: stat.stx_size,
                alloc: stat.stx_blocks * 512,
                files: 0,
                dirs: 1,
                mtime: reader::mtime_secs(stat.stx_mtime.tv_sec),
            };
            return Ok((totals, linux::mount_id(&stat)));
        }
    }
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a folder"));
    }
    let mut raw = RawEntry::default();
    portable_fields(&metadata, &mut raw);
    let totals = Totals {
        size: 0,
        alloc: raw.alloc,
        files: 0,
        dirs: 1,
        mtime: raw.mtime,
    };
    Ok((totals, raw.mount))
}

#[cfg(unix)]
fn portable_fields(metadata: &std::fs::Metadata, raw: &mut RawEntry) {
    use std::os::unix::fs::MetadataExt;
    raw.alloc = metadata.blocks() * 512;
    raw.mount = metadata.dev();
    raw.mtime = reader::mtime_secs(metadata.mtime());
}

#[cfg(not(unix))]
fn portable_fields(metadata: &std::fs::Metadata, raw: &mut RawEntry) {
    raw.mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| reader::mtime_secs(d.as_secs() as i64))
        .unwrap_or(0);
}

pub fn pseudo_mounts() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        return [
            "/System/Volumes",
            "/Volumes",
            "/dev",
            "/net",
            "/home",
            "/private/var/vm",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
    }
    const PSEUDO: &[&str] = &[
        "proc",
        "sysfs",
        "devtmpfs",
        "devpts",
        "cgroup",
        "cgroup2",
        "securityfs",
        "debugfs",
        "tracefs",
        "configfs",
        "fusectl",
        "bpf",
        "pstore",
        "efivarfs",
        "mqueue",
        "hugetlbfs",
        "autofs",
        "binfmt_misc",
        "rpc_pipefs",
        "nsfs",
    ];
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _device = fields.next()?;
            let mount = fields.next()?;
            let kind = fields.next()?;
            PSEUDO
                .contains(&kind)
                .then(|| PathBuf::from(mount.replace("\\040", " ")))
        })
        .collect()
}

pub fn start_scan(options: ScanOptions) -> (SharedTree, Arc<ScanProgress>) {
    let tree = Arc::new(RwLock::new(Tree::new(&options.root)));
    let progress = Arc::new(ScanProgress::new(0));
    let shared = tree.clone();
    let handle = progress.clone();
    std::thread::Builder::new()
        .name("dirstat-scan".into())
        .spawn(move || run_root(&shared, 0, options, &handle))
        .ok();
    (tree, progress)
}

pub fn start_rescan(tree: &SharedTree, node: NodeId, mut options: ScanOptions) -> Arc<ScanProgress> {
    options.root = tree.read().path(node);
    let progress = Arc::new(ScanProgress::new(node));
    let shared = tree.clone();
    let handle = progress.clone();
    std::thread::Builder::new()
        .name("dirstat-rescan".into())
        .spawn(move || run_root(&shared, node, options, &handle))
        .ok();
    progress
}

pub fn scan_blocking(options: ScanOptions) -> Result<(Tree, Arc<ScanProgress>), String> {
    let tree = Arc::new(RwLock::new(Tree::new(&options.root)));
    let progress = Arc::new(ScanProgress::new(0));
    run_root(&tree, 0, options, &progress);
    if let Some(failure) = progress.failure() {
        return Err(failure);
    }
    let tree = Arc::try_unwrap(tree)
        .map(RwLock::into_inner)
        .map_err(|_| "scan still running".to_string())?;
    Ok((tree, progress))
}

fn run_root(tree: &SharedTree, node: NodeId, options: ScanOptions, progress: &Arc<ScanProgress>) {
    let (own, dev) = match root_info(&options.root) {
        Ok(info) => info,
        Err(error) => {
            if node != 0 && error.kind() == std::io::ErrorKind::NotFound {
                tree.write().remove(node);
            }
            progress.fail(format!("{}: {error}", options.root.display()));
            return;
        }
    };
    {
        let mut guard = tree.write();
        guard.reset_dir(node, own);
        guard.sorted = None;
    }
    progress.dirs.store(1, Ordering::Relaxed);
    let (sender, receiver) = bounded(4096);
    let collector = {
        let tree = tree.clone();
        let progress = progress.clone();
        let dedup = options.dedup_hardlinks;
        let metric = options.metric;
        std::thread::Builder::new()
            .name("dirstat-collect".into())
            .spawn(move || collect::run(&tree, receiver, node, &progress, dedup, metric))
    };
    let skip = if options.cross_filesystems {
        pseudo_mounts()
    } else {
        Vec::new()
    };
    let context = walk::WalkContext {
        sender,
        next_key: AtomicU32::new(1),
        progress,
        excludes: &options.excludes,
        cross_filesystems: options.cross_filesystems,
        skip: &skip,
    };
    let root = options.root.clone();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.thread_count())
        .thread_name(|index| format!("dirstat-walk-{index}"))
        .build();
    let pool_error = match pool {
        Ok(pool) => {
            pool.scope(|scope| walk::scan_dir(scope, &context, 0, root, dev));
            None
        }
        Err(error) => {
            progress.cancel();
            Some(error.to_string())
        }
    };
    drop(context);
    match collector {
        Ok(handle) => {
            if handle.join().is_err() {
                progress.fail("the scan stopped unexpectedly".into());
            }
        }
        Err(error) => progress.fail(error.to_string()),
    }
    if let Some(error) = pool_error {
        progress.fail(format!("could not start the scanner threads: {error}"));
    }
}
