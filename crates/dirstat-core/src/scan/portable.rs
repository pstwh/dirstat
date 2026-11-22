use std::path::Path;

use super::reader::{DirBatch, RawEntry, ReadContext, mtime_secs};
use crate::tree::NodeFlags;

#[cfg(unix)]
fn platform_fields(metadata: &std::fs::Metadata, entry: &mut RawEntry) {
    use std::os::unix::fs::MetadataExt;
    entry.alloc = metadata.blocks() * 512;
    entry.ino = metadata.ino();
    entry.dev = metadata.dev();
    entry.mount = metadata.dev();
    entry.shared = metadata.nlink() > 1 && !metadata.is_dir();
    entry.mtime = mtime_secs(metadata.mtime());
}

#[cfg(not(unix))]
fn platform_fields(metadata: &std::fs::Metadata, entry: &mut RawEntry) {
    entry.alloc = entry.size.div_ceil(4096) * 4096;
    entry.mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| mtime_secs(d.as_secs() as i64))
        .unwrap_or(0);
}

pub fn read_dir(path: &Path, batch: &mut DirBatch, context: &ReadContext) -> std::io::Result<()> {
    for item in std::fs::read_dir(path)? {
        let entry = match item {
            Ok(entry) => entry,
            Err(error) => {
                batch.error = Some(error.kind());
                break;
            }
        };
        let file_name = entry.file_name();
        let name = file_name.as_encoded_bytes();
        if !context.excludes.is_empty() && context.excludes.matches(path, name) {
            continue;
        }
        let raw = match entry.metadata() {
            Ok(metadata) => {
                let kind = metadata.file_type();
                let mut flags = NodeFlags::default();
                if kind.is_symlink() {
                    flags.set(NodeFlags::SYMLINK);
                } else if kind.is_dir() {
                    flags.set(NodeFlags::DIR);
                } else if !kind.is_file() {
                    flags.set(NodeFlags::OTHER);
                }
                let mut raw = RawEntry {
                    size: if kind.is_dir() { 0 } else { metadata.len() },
                    mount: batch.mount,
                    flags,
                    ..RawEntry::default()
                };
                platform_fields(&metadata, &mut raw);
                raw
            }
            Err(_) => {
                let mut flags = NodeFlags(NodeFlags::ERROR);
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    flags.set(NodeFlags::DIR);
                }
                RawEntry {
                    mount: batch.mount,
                    flags,
                    ..RawEntry::default()
                }
            }
        };
        batch.push(name, raw);
    }
    Ok(())
}
