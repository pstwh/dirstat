use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use parking_lot::RwLock;
use rustc_hash::{FxHashMap, FxHashSet};

use super::reader::DirBatch;
use super::{ScanProgress, ScanState};
use crate::tree::{Metric, NONE, Node, NodeFlags, NodeId, Totals, Tree, push_name};

const BURST: Duration = Duration::from_millis(3);
const BURST_BATCHES: usize = 512;

struct Collector<'a> {
    progress: &'a ScanProgress,
    key_to_node: Vec<NodeId>,
    pending: FxHashMap<u32, DirBatch>,
    seen: FxHashSet<(u64, u64)>,
    dedup: bool,
    last_parent: NodeId,
}

pub fn run(
    tree: &RwLock<Tree>,
    receiver: Receiver<DirBatch>,
    root: NodeId,
    progress: &Arc<ScanProgress>,
    dedup: bool,
    metric: Metric,
) {
    let mut collector = Collector {
        progress,
        key_to_node: vec![root],
        pending: FxHashMap::default(),
        seen: FxHashSet::default(),
        dedup,
        last_parent: root,
    };
    let mut last_path = Instant::now();
    let mut last_colors = Instant::now();
    while let Ok(first) = receiver.recv() {
        let mut guard = tree.write();
        let started = Instant::now();
        collector.apply(&mut guard, first);
        let mut count = 1;
        while count < BURST_BATCHES && started.elapsed() < BURST {
            match receiver.try_recv() {
                Ok(batch) => collector.apply(&mut guard, batch),
                Err(_) => break,
            }
            count += 1;
        }
        if last_path.elapsed() > Duration::from_millis(100) {
            *progress.current.lock() = guard.path(collector.last_parent);
            last_path = Instant::now();
        }
        if last_colors.elapsed() > Duration::from_millis(400) {
            guard.exts.assign_colors(metric);
            guard.generation += 1;
            last_colors = Instant::now();
        }
    }
    progress.set_state(ScanState::Finalizing);
    {
        let mut guard = tree.write();
        guard.sort_all(metric);
        guard.exts.assign_colors(metric);
        guard.nodes.shrink_to_fit();
        guard.order.shrink_to_fit();
        guard.names.shrink_to_fit();
        guard.generation += 1;
    }
    progress.set_state(if progress.is_cancelled() {
        ScanState::Cancelled
    } else {
        ScanState::Done
    });
}

impl Collector<'_> {
    fn apply(&mut self, tree: &mut Tree, batch: DirBatch) {
        let parent = match self.key_to_node.get(batch.key as usize) {
            Some(&node) if node != NONE => node,
            _ => {
                self.pending.insert(batch.key, batch);
                return;
            }
        };
        let first = tree.nodes.len() as u32;
        let depth = tree.nodes[parent as usize].depth.saturating_add(1);
        let mut totals = Totals::default();
        let mut mapped: Vec<u32> = Vec::new();
        tree.nodes.reserve(batch.entries.len());
        for entry in &batch.entries {
            let name = batch.name(entry);
            let name_off = push_name(&mut tree.names, name);
            let id = tree.nodes.len() as u32;
            let mut flags = entry.flags;
            totals.mtime = totals.mtime.max(entry.mtime);
            if entry.is_dir() {
                if entry.child_key != 0 {
                    let key = entry.child_key as usize;
                    if self.key_to_node.len() <= key {
                        self.key_to_node.resize(key + 1, NONE);
                    }
                    self.key_to_node[key] = id;
                    mapped.push(entry.child_key);
                }
                totals.size += entry.size;
                totals.alloc += entry.alloc;
                totals.dirs += 1;
                tree.nodes.push(Node {
                    size: entry.size,
                    alloc: entry.alloc,
                    parent,
                    name_off,
                    dirs: 1,
                    mtime: entry.mtime,
                    flags,
                    depth,
                    ..Node::default()
                });
            } else {
                if self.dedup && entry.shared && !self.seen.insert((entry.dev, entry.ino)) {
                    flags.set(NodeFlags::HARDLINK);
                }
                let ext = tree.exts.intern_name(name);
                if !flags.has(NodeFlags::HARDLINK) {
                    tree.exts.add(ext, entry.size, entry.alloc);
                    totals.size += entry.size;
                    totals.alloc += entry.alloc;
                }
                totals.files += 1;
                tree.nodes.push(Node {
                    size: entry.size,
                    alloc: entry.alloc,
                    parent,
                    name_off,
                    files: 1,
                    mtime: entry.mtime,
                    ext,
                    flags,
                    depth,
                    ..Node::default()
                });
            }
        }
        let count = batch.entries.len() as u32;
        let node = &mut tree.nodes[parent as usize];
        node.first_child = first;
        node.child_count = count;
        tree.order.extend(first..first + count);
        tree.add_up(parent, totals);
        if let Some(kind) = batch.error {
            tree.nodes[parent as usize].flags.set(NodeFlags::ERROR);
            tree.errors.push((parent, kind));
            self.progress.errors.fetch_add(1, Ordering::Relaxed);
        }
        self.progress.files.fetch_add(totals.files as u64, Ordering::Relaxed);
        self.progress.dirs.fetch_add(totals.dirs as u64, Ordering::Relaxed);
        self.progress.bytes.fetch_add(totals.size, Ordering::Relaxed);
        self.progress.alloc.fetch_add(totals.alloc, Ordering::Relaxed);
        self.last_parent = parent;
        if !self.pending.is_empty() {
            for key in mapped {
                if let Some(waiting) = self.pending.remove(&key) {
                    self.apply(tree, waiting);
                }
            }
        }
    }
}
