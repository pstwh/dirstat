use std::collections::HashSet;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use dirstat_core::disks::{self, Disk};
use dirstat_core::dupes::{DupGroup, DupProgress, find_duplicates};
use dirstat_core::format::{human_count, human_duration, human_size};
use dirstat_core::fsops;
use dirstat_core::{Metric, NONE, NodeId, ScanProgress, ScanState, SharedTree, Tree};
use egui::Context;
use parking_lot::RwLock;

use crate::palette::Palette;
use crate::settings::{Appearance, Settings};
use crate::theme::Theme;
use crate::treemap_view::TreemapView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Good,
    Bad,
}

pub struct Toast {
    pub text: String,
    pub tone: Tone,
    pub until: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Folders,
    Types,
    Largest,
    Duplicates,
    Search,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Name,
    Size,
    Other,
    Items,
    Files,
    Modified,
}

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub node: NodeId,
    pub depth: u16,
}

pub enum Dialog {
    Remove {
        nodes: Vec<NodeId>,
        paths: Vec<PathBuf>,
        bytes: u64,
        permanent: bool,
    },
    Errors,
    Settings {
        excludes: String,
    },
    Keys,
}

#[derive(Clone, Debug)]
pub enum Action {
    Select(NodeId),
    Open(NodeId),
    Reveal(NodeId),
    CopyPath(NodeId),
    CopyName(NodeId),
    Zoom(NodeId),
    ZoomOut,
    ZoomRoot,
    Rescan(NodeId),
    Trash(Vec<NodeId>),
    Delete(Vec<NodeId>),
    Exclude(NodeId),
    Highlight(Option<u16>),
    SearchFor(String),
    Expand(NodeId, bool),
}

pub enum Job {
    Removed {
        tree: SharedTree,
        removed: Vec<NodeId>,
        failed: Vec<(PathBuf, String)>,
        bytes: u64,
        permanent: bool,
    },
    Exported(Result<PathBuf, String>),
    Imported(Result<(Tree, PathBuf), String>),
    Dupes(Vec<DupGroup>),
}

pub struct FolderState {
    pub expanded: HashSet<NodeId>,
    pub rows: Vec<Row>,
    pub dirty: bool,
    pub built: Instant,
    pub built_generation: u64,
    pub sort: Column,
    pub descending: bool,
    pub scroll_to: Option<NodeId>,
}

impl Default for FolderState {
    fn default() -> Self {
        FolderState {
            expanded: HashSet::from([0]),
            rows: Vec::new(),
            dirty: true,
            built: Instant::now(),
            built_generation: u64::MAX,
            sort: Column::Size,
            descending: true,
            scroll_to: None,
        }
    }
}

#[derive(Default)]
pub struct SearchState {
    pub text: String,
    pub results: Vec<NodeId>,
    pub ran: Option<(String, u64, NodeId)>,
    pub took: Duration,
    pub focus: bool,
    pub edited: Option<Instant>,
}

#[derive(Default)]
pub struct LargestState {
    pub items: Vec<NodeId>,
    pub key: Option<(u64, NodeId, Metric, bool)>,
}

#[derive(Default)]
pub struct DupesState {
    pub progress: Option<Arc<DupProgress>>,
    pub groups: Vec<DupGroup>,
    pub checked: HashSet<NodeId>,
    pub collapsed: HashSet<usize>,
    pub ran: bool,
    pub took: Duration,
    pub started: Option<Instant>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Scan(PathBuf),
    Import(PathBuf),
}

pub struct App {
    pub theme: Theme,
    pub settings: Settings,
    pub disks: Vec<Disk>,
    pub disks_at: Instant,
    pub tree: Option<SharedTree>,
    pub job: Option<Arc<ScanProgress>>,
    pub source: Option<Source>,
    pub zoom: NodeId,
    pub selected: Option<NodeId>,
    pub hovered: Option<NodeId>,
    pub last_hovered: Option<NodeId>,
    pub highlight_ext: Option<u16>,
    pub section: Section,
    pub folders: FolderState,
    pub search: SearchState,
    pub largest: LargestState,
    pub dupes: DupesState,
    pub treemap: TreemapView,
    pub palette: Option<Palette>,
    pub dialog: Option<Dialog>,
    pub toasts: Vec<Toast>,
    pub actions: Vec<Action>,
    pub jobs: Sender<Job>,
    pub job_results: Receiver<Job>,
    pub settings_dirty: Option<Instant>,
    pub job_reported: bool,
    pub removing: usize,
    pub deferred: Vec<Job>,
    pub pending_restore: Option<(Option<PathBuf>, Option<PathBuf>)>,
}

impl App {
    pub fn new(ctx: &Context) -> App {
        let settings = Settings::load();
        let (jobs, job_results) = unbounded();
        App {
            theme: Theme::default(),
            settings,
            disks: disks::list(),
            disks_at: Instant::now(),
            tree: None,
            job: None,
            source: None,
            zoom: 0,
            selected: None,
            hovered: None,
            last_hovered: None,
            highlight_ext: None,
            section: Section::Folders,
            folders: FolderState::default(),
            search: SearchState::default(),
            largest: LargestState::default(),
            dupes: DupesState::default(),
            treemap: TreemapView::new(ctx),
            palette: None,
            dialog: None,
            toasts: Vec::new(),
            actions: Vec::new(),
            jobs,
            job_results,
            settings_dirty: None,
            job_reported: true,
            removing: 0,
            deferred: Vec::new(),
            pending_restore: None,
        }
    }

    pub fn sync_theme(&mut self, ctx: &Context) {
        let dark = match self.settings.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => ctx.system_theme().is_none_or(|theme| theme == egui::Theme::Dark),
        };
        if dark != self.theme.dark {
            self.theme = Theme::new(dark, 13.0);
            self.theme.install(ctx);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, tone: Tone) {
        let text = text.into();
        let until = Instant::now() + Duration::from_millis(3600);
        if let Some(existing) = self.toasts.iter_mut().find(|t| t.text == text) {
            existing.until = until;
            return;
        }
        self.toasts.push(Toast { text, tone, until });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    pub fn save_settings_soon(&mut self) {
        self.settings_dirty = Some(Instant::now());
    }

    pub fn is_scanning(&self) -> bool {
        self.job.as_ref().is_some_and(|job| job.is_running())
    }

    pub fn has_tree(&self) -> bool {
        self.tree.is_some()
    }

    pub fn shared(&self) -> Option<SharedTree> {
        self.tree.clone()
    }

    pub fn root_path(&self) -> Option<PathBuf> {
        self.tree.as_ref().map(|tree| tree.read().root_path.clone())
    }

    pub fn refresh_disks(&mut self) {
        self.disks = disks::list();
        self.disks_at = Instant::now();
    }

    pub fn choose_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Choose a folder to scan");
        if let Some(path) = self.root_path() {
            dialog = dialog.set_directory(path);
        }
        if let Some(path) = dialog.pick_folder() {
            self.scan(path);
        }
    }

    fn reset_view(&mut self) {
        self.zoom = 0;
        self.selected = None;
        self.hovered = None;
        self.highlight_ext = None;
        self.folders = FolderState::default();
        self.search.results.clear();
        self.search.ran = None;
        self.largest = LargestState::default();
        if let Some(progress) = &self.dupes.progress {
            progress.cancel.store(true, Ordering::Relaxed);
        }
        self.dupes = DupesState::default();
        self.treemap.reset();
    }

    pub fn scan(&mut self, path: PathBuf) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let path = clean_path(path);
        let (tree, progress) = dirstat_core::start_scan(self.settings.scan_options(&path));
        self.tree = Some(tree);
        self.job = Some(progress);
        self.job_reported = false;
        self.source = Some(Source::Scan(path.clone()));
        self.reset_view();
        self.settings.remember(&path);
        self.save_settings_soon();
    }

    pub fn rescan_all(&mut self) {
        if self.removing > 0 {
            self.toast("Wait for the files to be removed", Tone::Info);
            return;
        }
        match self.source.clone() {
            Some(Source::Scan(path)) => {
                let keep = self.selected.and_then(|id| self.path_of(id));
                let zoom = self.path_of(self.zoom);
                self.scan(path);
                self.pending_restore = None;
                if keep.is_some() || zoom.is_some() {
                    self.pending_restore = Some((zoom, keep));
                }
            }
            Some(Source::Import(_)) => self.toast("An imported scan cannot be refreshed", Tone::Info),
            None => self.choose_folder(),
        }
    }

    pub fn path_of(&self, node: NodeId) -> Option<PathBuf> {
        let tree = self.tree.as_ref()?.read();
        tree.get(node).map(|_| tree.path(node))
    }

    pub fn rescan_node(&mut self, node: NodeId) {
        if self.removing > 0 {
            self.toast("Wait for the files to be removed", Tone::Info);
            return;
        }
        if self.is_scanning() {
            self.toast("Wait for the current scan to finish", Tone::Info);
            return;
        }
        if !matches!(self.source, Some(Source::Scan(_))) {
            self.toast("An imported scan cannot be refreshed", Tone::Info);
            return;
        }
        let Some(tree) = self.shared() else { return };
        let target = {
            let guard = tree.read();
            let mut target = node;
            if !guard.node(target).is_dir() {
                target = guard.node(target).parent;
            }
            target
        };
        if target == NONE {
            return;
        }
        if target == 0 {
            self.rescan_all();
            return;
        }
        let root = tree.read().root_path.clone();
        let progress = dirstat_core::start_rescan(&tree, target, self.settings.scan_options(&root));
        self.job = Some(progress);
        self.job_reported = false;
        self.folders.dirty = true;
    }

    pub fn stop(&mut self) {
        if let Some(job) = &self.job
            && job.is_running()
        {
            job.cancel();
            self.toast("Stopping the scan", Tone::Info);
        }
    }

    pub fn select(&mut self, node: NodeId, reveal: bool) {
        self.selected = Some(node);
        if let Some(tree) = self.shared() {
            let tree = tree.read();
            if tree.get(node).is_none() {
                return;
            }
            for ancestor in tree.ancestors(node) {
                if ancestor != node && self.folders.expanded.insert(ancestor) {
                    self.folders.dirty = true;
                }
            }
        }
        if reveal {
            self.folders.scroll_to = Some(node);
        }
    }

    pub fn zoom_to(&mut self, node: NodeId) {
        let Some(tree) = self.shared() else { return };
        let tree = tree.read();
        let Some(entry) = tree.get(node) else { return };
        self.zoom = if entry.is_dir() { node } else { entry.parent };
    }

    pub fn zoom_out(&mut self) {
        let Some(tree) = self.shared() else { return };
        let parent = tree.read().node(self.zoom).parent;
        if parent != NONE {
            let previous = self.zoom;
            self.zoom = parent;
            if self.selected.is_none() {
                self.selected = Some(previous);
            }
        }
    }

    pub fn selection_or_zoom(&self) -> NodeId {
        self.selected.unwrap_or(self.zoom)
    }

    pub fn request_remove(&mut self, nodes: Vec<NodeId>, permanent: bool) {
        if self.is_scanning() {
            self.toast("Wait for the scan to finish before removing files", Tone::Info);
            return;
        }
        if matches!(self.source, Some(Source::Import(_))) {
            self.toast("Files of an imported scan cannot be removed from here", Tone::Info);
            return;
        }
        let Some(tree) = self.shared() else { return };
        let tree = tree.read();
        let mut unique: Vec<NodeId> = nodes
            .into_iter()
            .filter(|&n| n != tree.root && tree.get(n).is_some())
            .collect();
        unique.sort_unstable();
        unique.dedup();
        let snapshot = unique.clone();
        unique.retain(|&n| !snapshot.iter().any(|&other| other != n && tree.is_ancestor(other, n)));
        if unique.is_empty() {
            return;
        }
        let bytes = unique.iter().map(|&n| tree.weight(n, self.settings.metric)).sum();
        let paths = unique.iter().map(|&n| tree.path(n)).collect();
        drop(tree);
        if !permanent && !self.settings.confirm_trash {
            self.perform_remove(unique, paths, bytes, permanent);
            return;
        }
        self.dialog = Some(Dialog::Remove {
            nodes: unique,
            paths,
            bytes,
            permanent,
        });
    }

    pub fn perform_remove(&mut self, nodes: Vec<NodeId>, paths: Vec<PathBuf>, bytes: u64, permanent: bool) {
        let Some(tree) = self.shared() else { return };
        self.removing += 1;
        let sender = self.jobs.clone();
        std::thread::spawn(move || {
            let mut removed = Vec::new();
            let mut failed = Vec::new();
            if permanent {
                for (node, path) in nodes.iter().zip(paths.iter()) {
                    match fsops::delete(path) {
                        Ok(()) => removed.push(*node),
                        Err(error) => failed.push((path.clone(), error)),
                    }
                }
            } else {
                match fsops::trash(&paths) {
                    Ok(()) => removed = nodes.clone(),
                    Err(_) => {
                        for (node, path) in nodes.iter().zip(paths.iter()) {
                            match fsops::trash(std::slice::from_ref(path)) {
                                Ok(()) => removed.push(*node),
                                Err(_) if !fsops::exists(path) => removed.push(*node),
                                Err(error) => failed.push((path.clone(), error)),
                            }
                        }
                    }
                }
            }
            let _ = sender.send(Job::Removed {
                tree,
                removed,
                failed,
                bytes,
                permanent,
            });
        });
        self.toast(if permanent { "Deleting..." } else { "Moving to trash..." }, Tone::Info);
    }

    pub fn export(&mut self, json: bool) {
        if self.is_scanning() {
            self.toast("Wait for the scan to finish before exporting", Tone::Info);
            return;
        }
        let Some(tree) = self.shared() else {
            self.toast("Scan something first", Tone::Info);
            return;
        };
        let name = if json { "dirstat-scan.json" } else { "dirstat-scan.csv" };
        let filter = if json { ("ncdu JSON", "json") } else { ("CSV", "csv") };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(name)
            .add_filter(filter.0, &[filter.1])
            .save_file()
        else {
            return;
        };
        let metric = self.settings.metric;
        let root = self.zoom;
        let sender = self.jobs.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
                let mut out = BufWriter::with_capacity(1 << 20, file);
                let tree = tree.read();
                if json {
                    dirstat_core::export::write_ncdu(&tree, root, &mut out).map_err(|e| e.to_string())?;
                } else {
                    dirstat_core::export::write_csv(&tree, root, metric, &mut out).map_err(|e| e.to_string())?;
                }
                std::io::Write::flush(&mut out).map_err(|e| e.to_string())?;
                Ok(path)
            })();
            let _ = sender.send(Job::Exported(result));
        });
        self.toast("Exporting...", Tone::Info);
    }

    pub fn import_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("ncdu JSON", &["json"]).pick_file() {
            self.import(path);
        }
    }

    pub fn import(&mut self, path: PathBuf) {
        let sender = self.jobs.clone();
        std::thread::spawn(move || {
            let result = std::fs::File::open(&path)
                .map_err(|e| e.to_string())
                .and_then(|file| dirstat_core::export::read_ncdu(BufReader::with_capacity(1 << 20, file)))
                .map(|tree| (tree, path));
            let _ = sender.send(Job::Imported(result));
        });
        self.toast("Opening the saved scan...", Tone::Info);
    }

    pub fn find_duplicates(&mut self) {
        if self.is_scanning() {
            self.toast("Wait for the scan to finish", Tone::Info);
            return;
        }
        let Some(tree) = self.shared() else { return };
        if let Some(progress) = &self.dupes.progress {
            progress.cancel.store(true, Ordering::Relaxed);
        }
        let progress = Arc::new(DupProgress::default());
        let ssd = self
            .root_path()
            .and_then(|root| disks::containing(&self.disks, &root).and_then(|d| d.ssd))
            .unwrap_or(true);
        let threads = if ssd { 8 } else { 1 };
        let min_size = self.settings.dupes_min_size;
        let sender = self.jobs.clone();
        let handle = progress.clone();
        std::thread::spawn(move || {
            let groups = find_duplicates(&tree, min_size, threads, &handle);
            if !handle.cancel.load(Ordering::Relaxed) {
                let _ = sender.send(Job::Dupes(groups));
            }
        });
        self.dupes.progress = Some(progress);
        self.dupes.started = Some(Instant::now());
        self.dupes.checked.clear();
        self.section = Section::Duplicates;
        self.settings.show_sidebar = true;
    }

    pub fn cancel_duplicates(&mut self) {
        if let Some(progress) = self.dupes.progress.take() {
            progress.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn tick(&mut self, ctx: &Context) {
        let now = Instant::now();
        self.toasts.retain(|t| t.until > now);
        if !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if let Some(at) = self.settings_dirty
            && now.duration_since(at) > Duration::from_millis(500)
        {
            self.settings.save();
            self.settings_dirty = None;
        }
        if self.settings_dirty.is_some() {
            ctx.request_repaint_after(Duration::from_millis(550));
        }
        if self.tree.is_none() && now.duration_since(self.disks_at) > Duration::from_secs(5) {
            self.refresh_disks();
        }
        if self.tree.is_none() {
            ctx.request_repaint_after(Duration::from_secs(5));
        }
        self.poll_job(ctx);
        for result in std::mem::take(&mut self.deferred) {
            self.finish_job(result);
        }
        if !self.deferred.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        while let Ok(result) = self.job_results.try_recv() {
            self.finish_job(result);
        }
        if self.dupes.progress.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.restore_after_scan();
    }

    fn poll_job(&mut self, ctx: &Context) {
        let Some(job) = self.job.clone() else { return };
        if job.is_running() {
            ctx.request_repaint_after(Duration::from_millis(80));
            return;
        }
        if self.job_reported {
            return;
        }
        self.job_reported = true;
        let state = job.state();
        let entries = job.entries();
        let elapsed = human_duration(job.elapsed());
        self.folders.dirty = true;
        match state {
            ScanState::Done if job.target == 0 => {
                self.toast(
                    format!("Scanned {} items in {elapsed}", human_count(entries)),
                    Tone::Good,
                );
            }
            ScanState::Done => self.toast(format!("Folder refreshed in {elapsed}"), Tone::Good),
            ScanState::Cancelled => self.toast("Scan stopped, showing what was found", Tone::Info),
            ScanState::Failed => {
                let message = job.failure().unwrap_or_else(|| "The scan failed".into());
                self.toast(message, Tone::Bad);
                if job.target == 0 {
                    self.tree = None;
                    self.source = None;
                }
            }
            _ => {}
        }
        self.repair_view();
    }

    fn restore_after_scan(&mut self) {
        if self.is_scanning() {
            return;
        }
        let Some((zoom, selected)) = self.pending_restore.take() else {
            return;
        };
        let Some(tree) = self.shared() else { return };
        let (zoom, selected) = {
            let tree = tree.read();
            (
                zoom.and_then(|p| tree.find_path(&p)),
                selected.and_then(|p| tree.find_path(&p)),
            )
        };
        if let Some(zoom) = zoom {
            self.zoom = zoom;
        }
        if let Some(selected) = selected {
            self.select(selected, true);
        }
    }

    pub fn repair_view(&mut self) {
        let Some(tree) = self.shared() else { return };
        let tree = tree.read();
        let mut zoom = self.zoom;
        while zoom != NONE && tree.get(zoom).is_none() {
            zoom = tree.node(zoom).parent;
        }
        self.zoom = if zoom == NONE { 0 } else { zoom };
        if self.selected.is_some_and(|s| tree.get(s).is_none()) {
            self.selected = None;
        }
        if self.hovered.is_some_and(|s| tree.get(s).is_none()) {
            self.hovered = None;
        }
    }

    fn finish_job(&mut self, result: Job) {
        match result {
            Job::Removed {
                tree,
                removed,
                failed,
                bytes,
                permanent,
            } => {
                let current = self.tree.as_ref().is_some_and(|t| Arc::ptr_eq(t, &tree));
                if current {
                    let Some(mut guard) = tree.try_write_for(Duration::from_millis(4)) else {
                        self.deferred.push(Job::Removed {
                            tree: tree.clone(),
                            removed,
                            failed,
                            bytes,
                            permanent,
                        });
                        return;
                    };
                    for &node in &removed {
                        guard.remove(node);
                    }
                    drop(guard);
                    let gone: HashSet<NodeId> = removed.iter().copied().collect();
                    self.dupes.checked.retain(|node| !gone.contains(node));
                    for group in &mut self.dupes.groups {
                        let keep: Vec<usize> = (0..group.nodes.len())
                            .filter(|&i| !gone.contains(&group.nodes[i]))
                            .collect();
                        group.paths = keep.iter().map(|&i| group.paths[i].clone()).collect();
                        group.nodes = keep.iter().map(|&i| group.nodes[i]).collect();
                    }
                    self.dupes.groups.retain(|g| g.nodes.len() > 1);
                }
                self.removing = self.removing.saturating_sub(1);
                self.toasts.retain(|t| t.tone != Tone::Info);
                self.folders.dirty = true;
                self.largest.key = None;
                self.search.ran = None;
                self.repair_view();
                if !removed.is_empty() {
                    let verb = if permanent { "Deleted" } else { "Moved to trash" };
                    let count = removed.len();
                    let what = if count == 1 {
                        "1 item".to_string()
                    } else {
                        format!("{count} items")
                    };
                    let freed = if failed.is_empty() {
                        format!(", freed {}", human_size(bytes))
                    } else {
                        String::new()
                    };
                    self.toast(format!("{verb} {what}{freed}"), Tone::Good);
                }
                if let Some((path, error)) = failed.first() {
                    let more = if failed.len() > 1 {
                        format!(" (and {} more)", failed.len() - 1)
                    } else {
                        String::new()
                    };
                    self.toast(format!("Could not remove {}: {error}{more}", path.display()), Tone::Bad);
                }
            }
            Job::Exported(Ok(path)) => {
                self.toasts.retain(|t| t.tone != Tone::Info);
                self.toast(format!("Exported to {}", path.display()), Tone::Good);
            }
            Job::Exported(Err(error)) => self.toast(format!("Export failed: {error}"), Tone::Bad),
            Job::Imported(Ok((tree, path))) => {
                self.toasts.retain(|t| t.tone != Tone::Info);
                if let Some(job) = &self.job {
                    job.cancel();
                }
                self.job = None;
                self.tree = Some(Arc::new(RwLock::new(tree)));
                self.source = Some(Source::Import(path.clone()));
                self.reset_view();
                self.toast(format!("Opened {}", path.display()), Tone::Good);
            }
            Job::Imported(Err(error)) => self.toast(format!("Could not open the scan: {error}"), Tone::Bad),
            Job::Dupes(groups) => {
                let took = self.dupes.started.map(|s| s.elapsed()).unwrap_or_default();
                self.dupes.progress = None;
                self.dupes.ran = true;
                self.dupes.took = took;
                self.dupes.collapsed.clear();
                let wasted: u64 = groups.iter().map(|g| g.wasted()).sum();
                if groups.is_empty() {
                    self.toast("No duplicates found", Tone::Info);
                } else {
                    self.toast(
                        format!(
                            "{} duplicate groups, {} could be freed",
                            human_count(groups.len() as u64),
                            human_size(wasted)
                        ),
                        Tone::Good,
                    );
                }
                self.dupes.groups = groups;
            }
        }
    }

    pub fn set_metric(&mut self, metric: Metric) {
        if self.settings.metric == metric {
            return;
        }
        self.settings.metric = metric;
        self.save_settings_soon();
        self.folders.dirty = true;
        self.largest.key = None;
        self.search.ran = None;
        if let Some(tree) = self.shared()
            && !self.is_scanning()
        {
            std::thread::spawn(move || {
                let mut tree = tree.write();
                tree.sort_all(metric);
                tree.exts.assign_colors(metric);
                tree.generation += 1;
            });
        }
    }

    pub fn apply_actions(&mut self, ctx: &Context) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            self.run(action, ctx);
        }
    }

    pub fn run(&mut self, action: Action, ctx: &Context) {
        match action {
            Action::Select(node) => self.select(node, true),
            Action::Open(node) => {
                if let Some(path) = self.path_of(node)
                    && let Err(error) = fsops::open_path(&path)
                {
                    self.toast(format!("Could not open: {error}"), Tone::Bad);
                }
            }
            Action::Reveal(node) => {
                if let Some(path) = self.path_of(node)
                    && let Err(error) = fsops::reveal(&path)
                {
                    self.toast(format!("Could not show the file: {error}"), Tone::Bad);
                }
            }
            Action::CopyPath(node) => {
                if let Some(path) = self.path_of(node) {
                    ctx.copy_text(path.display().to_string());
                    self.toast("Path copied", Tone::Good);
                }
            }
            Action::CopyName(node) => {
                if let Some(tree) = self.shared() {
                    let name = tree.read().display_name(node);
                    ctx.copy_text(name);
                    self.toast("Name copied", Tone::Good);
                }
            }
            Action::Zoom(node) => self.zoom_to(node),
            Action::ZoomOut => self.zoom_out(),
            Action::ZoomRoot => self.zoom = 0,
            Action::Rescan(node) => self.rescan_node(node),
            Action::Trash(nodes) => self.request_remove(nodes, false),
            Action::Delete(nodes) => self.request_remove(nodes, true),
            Action::Exclude(node) => {
                if let Some(tree) = self.shared() {
                    let name = tree.read().name_lossy(node).into_owned();
                    if !self.settings.excludes.contains(&name) {
                        self.settings.excludes.push(name.clone());
                        self.save_settings_soon();
                    }
                    self.toast(
                        format!("Excluding \"{name}\" from the next scans (F5 to rescan)"),
                        Tone::Info,
                    );
                }
            }
            Action::Highlight(ext) => {
                self.highlight_ext = if self.highlight_ext == ext { None } else { ext };
            }
            Action::SearchFor(text) => {
                self.search.text = text;
                self.search.edited = Some(Instant::now() - Duration::from_secs(1));
                self.section = Section::Search;
                self.settings.show_sidebar = true;
            }
            Action::Expand(node, open) => {
                let changed = if open {
                    self.folders.expanded.insert(node)
                } else {
                    self.folders.expanded.remove(&node)
                };
                if changed {
                    self.folders.dirty = true;
                }
            }
        }
    }

    pub fn status_summary(&self) -> Option<String> {
        let tree = self.tree.as_ref()?.read();
        let root = tree.node(0);
        Some(format!(
            "{} files  {} folders  {}",
            human_count(root.files as u64),
            human_count(root.folders() as u64),
            human_size(root.weight(self.settings.metric))
        ))
    }
}

pub fn clean_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(stripped) = text.strip_prefix(r"\\?\")
        && !stripped.starts_with("UNC\\")
    {
        return PathBuf::from(stripped);
    }
    path
}
