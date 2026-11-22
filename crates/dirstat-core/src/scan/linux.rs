use std::cell::RefCell;
use std::mem::MaybeUninit;
use std::os::fd::AsFd;
use std::path::Path;

use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags, RawDir, StatxFlags, openat, statx};

use super::reader::{DirBatch, RawEntry, ReadContext, mtime_secs};
use crate::tree::NodeFlags;

const BUFFER_SIZE: usize = 128 * 1024;

thread_local! {
    static BUFFER: RefCell<Vec<MaybeUninit<u8>>> = RefCell::new(vec![MaybeUninit::uninit(); BUFFER_SIZE]);
}

pub fn mount_id(stat: &rustix::fs::Statx) -> u64 {
    if stat.stx_mask & StatxFlags::MNT_ID.bits() != 0 {
        stat.stx_mnt_id | (1 << 63)
    } else {
        device(stat.stx_dev_major, stat.stx_dev_minor)
    }
}

pub fn device(major: u32, minor: u32) -> u64 {
    ((major as u64) << 32) | minor as u64
}

pub fn read_dir(path: &Path, batch: &mut DirBatch, context: &ReadContext) -> std::io::Result<()> {
    let fd = openat(
        CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let mask = StatxFlags::TYPE
        | StatxFlags::SIZE
        | StatxFlags::BLOCKS
        | StatxFlags::MTIME
        | StatxFlags::NLINK
        | StatxFlags::INO
        | StatxFlags::MNT_ID;
    let flags = AtFlags::SYMLINK_NOFOLLOW | AtFlags::STATX_DONT_SYNC;
    BUFFER.with_borrow_mut(|buffer| {
        let mut dir = RawDir::new(fd.as_fd(), buffer.as_mut_slice());
        while let Some(item) = dir.next() {
            let entry = match item {
                Ok(entry) => entry,
                Err(error) => {
                    batch.error = Some(std::io::Error::from(error).kind());
                    break;
                }
            };
            let c_name = entry.file_name();
            let name = c_name.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if !context.excludes.is_empty() && context.excludes.matches(path, name) {
                continue;
            }
            let raw = match statx(fd.as_fd(), c_name, flags, mask) {
                Ok(stat) => {
                    let kind = FileType::from_raw_mode(stat.stx_mode as _);
                    let mut node_flags = NodeFlags::default();
                    match kind {
                        FileType::Directory => node_flags.set(NodeFlags::DIR),
                        FileType::Symlink => node_flags.set(NodeFlags::SYMLINK),
                        FileType::RegularFile => {}
                        _ => node_flags.set(NodeFlags::OTHER),
                    }
                    RawEntry {
                        size: stat.stx_size,
                        alloc: stat.stx_blocks * 512,
                        ino: stat.stx_ino,
                        dev: device(stat.stx_dev_major, stat.stx_dev_minor),
                        mount: mount_id(&stat),
                        mtime: mtime_secs(stat.stx_mtime.tv_sec),
                        flags: node_flags,
                        shared: stat.stx_nlink > 1 && kind != FileType::Directory,
                        ..RawEntry::default()
                    }
                }
                Err(_) => {
                    let mut node_flags = NodeFlags(NodeFlags::ERROR);
                    if entry.file_type() == FileType::Directory {
                        node_flags.set(NodeFlags::DIR);
                    }
                    RawEntry {
                        mount: batch.mount,
                        flags: node_flags,
                        ..RawEntry::default()
                    }
                }
            };
            batch.push(name, raw);
        }
    });
    Ok(())
}
