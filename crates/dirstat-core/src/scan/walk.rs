use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crossbeam_channel::Sender;
use rayon::Scope;

use super::ScanProgress;
use super::reader::{self, DirBatch, ReadContext};
use crate::exclude::Excludes;
use crate::tree::NodeFlags;

pub struct WalkContext<'a> {
    pub sender: Sender<DirBatch>,
    pub next_key: AtomicU32,
    pub progress: &'a Arc<ScanProgress>,
    pub excludes: &'a Excludes,
    pub cross_filesystems: bool,
    pub skip: &'a [PathBuf],
}

pub fn scan_dir<'s>(scope: &Scope<'s>, context: &'s WalkContext<'s>, key: u32, path: PathBuf, mount: u64) {
    if context.progress.is_cancelled() {
        return;
    }
    let mut batch = DirBatch::new(key, mount);
    let read_context = ReadContext {
        excludes: context.excludes,
    };
    if let Err(error) = reader::read_dir(&path, &mut batch, &read_context) {
        batch.error = Some(error.kind());
    }
    let mut children: Vec<(u32, PathBuf, u64)> = Vec::new();
    for index in 0..batch.entries.len() {
        let entry = batch.entries[index];
        if !entry.is_dir() {
            continue;
        }
        let name = batch.name(&entry);
        let child_path = path.join(unsafe { OsStr::from_encoded_bytes_unchecked(name) });
        if entry.mount != mount {
            batch.entries[index].flags.set(NodeFlags::MOUNT);
            if !context.cross_filesystems || is_skipped(context.skip, &child_path) {
                continue;
            }
        }
        let child_key = context.next_key.fetch_add(1, Ordering::Relaxed);
        batch.entries[index].child_key = child_key;
        children.push((child_key, child_path, entry.mount));
    }
    if context.sender.send(batch).is_err() {
        return;
    }
    for (child_key, child_path, child_dev) in children {
        scope.spawn(move |scope| scan_dir(scope, context, child_key, child_path, child_dev));
    }
}

fn is_skipped(skip: &[PathBuf], path: &Path) -> bool {
    skip.iter().any(|p| path.starts_with(p))
}
