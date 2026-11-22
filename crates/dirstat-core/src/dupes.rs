use std::cmp::Reverse;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use parking_lot::RwLock;
use rayon::prelude::*;

use crate::tree::{NodeFlags, NodeId, Tree};

const EDGE: usize = 16 * 1024;
const CHUNK: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct DupGroup {
    pub size: u64,
    pub nodes: Vec<NodeId>,
    pub paths: Vec<PathBuf>,
}

impl DupGroup {
    pub fn wasted(&self) -> u64 {
        self.size * (self.nodes.len() as u64).saturating_sub(1)
    }
}

#[derive(Default)]
pub struct DupProgress {
    pub stage: AtomicU8,
    pub files_done: AtomicU64,
    pub files_total: AtomicU64,
    pub bytes_done: AtomicU64,
    pub bytes_total: AtomicU64,
    pub cancel: AtomicBool,
    pub finished: AtomicBool,
}

impl DupProgress {
    pub const COLLECTING: u8 = 0;
    pub const SAMPLING: u8 = 1;
    pub const HASHING: u8 = 2;

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

struct Candidate {
    size: u64,
    node: NodeId,
    path: PathBuf,
    hash: [u8; 32],
}

fn sample_hash(path: &Path, size: u64) -> std::io::Result<([u8; 32], bool)> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    if size as usize <= EDGE * 2 {
        let mut buffer = Vec::with_capacity(size as usize);
        file.read_to_end(&mut buffer)?;
        hasher.update(&buffer);
        return Ok((*hasher.finalize().as_bytes(), true));
    }
    let mut buffer = vec![0u8; EDGE];
    file.read_exact(&mut buffer)?;
    hasher.update(&buffer);
    file.seek(SeekFrom::Start(size - EDGE as u64))?;
    file.read_exact(&mut buffer)?;
    hasher.update(&buffer);
    Ok((*hasher.finalize().as_bytes(), false))
}

fn full_hash(path: &Path, progress: &DupProgress) -> std::io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        if progress.cancelled() {
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        progress.bytes_done.fetch_add(read as u64, Ordering::Relaxed);
    }
    Ok(*hasher.finalize().as_bytes())
}

fn regroup(mut items: Vec<Candidate>) -> Vec<Vec<Candidate>> {
    items.sort_unstable_by(|a, b| b.size.cmp(&a.size).then(a.hash.cmp(&b.hash)));
    let mut groups: Vec<Vec<Candidate>> = Vec::new();
    let mut current: Vec<Candidate> = Vec::new();
    for item in items {
        if current
            .last()
            .is_some_and(|last| last.size != item.size || last.hash != item.hash)
        {
            let done = std::mem::take(&mut current);
            if done.len() > 1 {
                groups.push(done);
            }
        }
        current.push(item);
    }
    if current.len() > 1 {
        groups.push(current);
    }
    groups
}

pub fn find_duplicates(tree: &RwLock<Tree>, min_size: u64, threads: usize, progress: &DupProgress) -> Vec<DupGroup> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .thread_name(|i| format!("dirstat-hash-{i}"))
        .build();
    let run = || find_in_pool(tree, min_size.max(1), progress);
    let groups = match pool {
        Ok(pool) => pool.install(run),
        Err(_) => run(),
    };
    progress.finished.store(true, Ordering::Release);
    groups
}

fn find_in_pool(tree: &RwLock<Tree>, min_size: u64, progress: &DupProgress) -> Vec<DupGroup> {
    progress.stage.store(DupProgress::COLLECTING, Ordering::Relaxed);
    let candidates: Vec<Candidate> = {
        let tree = tree.read();
        let skip = NodeFlags::DIR
            | NodeFlags::DEAD
            | NodeFlags::HARDLINK
            | NodeFlags::SYMLINK
            | NodeFlags::OTHER
            | NodeFlags::ERROR;
        let mut sized: Vec<(u64, NodeId)> = tree
            .nodes
            .par_iter()
            .enumerate()
            .filter(|(_, n)| n.flags.0 & skip == 0 && n.size >= min_size)
            .map(|(id, n)| (n.size, id as NodeId))
            .collect();
        sized.par_sort_unstable();
        let mut keep = Vec::new();
        let mut start = 0;
        while start < sized.len() {
            let mut end = start + 1;
            while end < sized.len() && sized[end].0 == sized[start].0 {
                end += 1;
            }
            if end - start > 1 {
                keep.extend_from_slice(&sized[start..end]);
            }
            start = end;
        }
        keep.into_par_iter()
            .map(|(size, node)| Candidate {
                size,
                node,
                path: tree.path(node),
                hash: [0; 32],
            })
            .collect()
    };
    progress.stage.store(DupProgress::SAMPLING, Ordering::Relaxed);
    progress.files_total.store(candidates.len() as u64, Ordering::Relaxed);
    let sampled: Vec<(Candidate, bool)> = candidates
        .into_par_iter()
        .filter_map(|mut candidate| {
            if progress.cancelled() {
                return None;
            }
            let result = sample_hash(&candidate.path, candidate.size).ok();
            progress.files_done.fetch_add(1, Ordering::Relaxed);
            let (hash, complete) = result?;
            candidate.hash = hash;
            Some((candidate, complete))
        })
        .collect();
    if progress.cancelled() {
        return Vec::new();
    }
    let (complete, partial): (Vec<_>, Vec<_>) = sampled.into_iter().partition(|(_, complete)| *complete);
    let mut finished: Vec<Vec<Candidate>> = regroup(complete.into_iter().map(|(c, _)| c).collect());
    let pending: Vec<Candidate> = regroup(partial.into_iter().map(|(c, _)| c).collect())
        .into_iter()
        .flatten()
        .collect();
    progress.stage.store(DupProgress::HASHING, Ordering::Relaxed);
    progress.files_done.store(0, Ordering::Relaxed);
    progress.files_total.store(pending.len() as u64, Ordering::Relaxed);
    progress
        .bytes_total
        .store(pending.iter().map(|c| c.size).sum(), Ordering::Relaxed);
    let hashed: Vec<Candidate> = pending
        .into_par_iter()
        .with_max_len(1)
        .filter_map(|mut candidate| {
            if progress.cancelled() {
                return None;
            }
            let hash = full_hash(&candidate.path, progress).ok();
            progress.files_done.fetch_add(1, Ordering::Relaxed);
            candidate.hash = hash?;
            Some(candidate)
        })
        .collect();
    if progress.cancelled() {
        return Vec::new();
    }
    finished.extend(regroup(hashed));
    let mut groups: Vec<DupGroup> = finished
        .into_iter()
        .map(|members| DupGroup {
            size: members[0].size,
            nodes: members.iter().map(|c| c.node).collect(),
            paths: members.into_iter().map(|c| c.path).collect(),
        })
        .collect();
    groups.sort_by_key(|g| Reverse(g.wasted()));
    groups
}
