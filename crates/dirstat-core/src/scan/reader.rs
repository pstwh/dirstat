use std::io::ErrorKind;
use std::path::Path;

use crate::exclude::Excludes;
use crate::tree::NodeFlags;

#[derive(Clone, Copy, Debug, Default)]
pub struct RawEntry {
    pub size: u64,
    pub alloc: u64,
    pub ino: u64,
    pub dev: u64,
    pub mount: u64,
    pub mtime: u32,
    pub name_off: u32,
    pub child_key: u32,
    pub name_len: u16,
    pub flags: NodeFlags,
    pub shared: bool,
}

impl RawEntry {
    pub fn is_dir(&self) -> bool {
        self.flags.has(NodeFlags::DIR)
    }
}

#[derive(Debug, Default)]
pub struct DirBatch {
    pub key: u32,
    pub mount: u64,
    pub entries: Vec<RawEntry>,
    pub names: Vec<u8>,
    pub error: Option<ErrorKind>,
}

impl DirBatch {
    pub fn new(key: u32, mount: u64) -> DirBatch {
        DirBatch {
            key,
            mount,
            entries: Vec::new(),
            names: Vec::new(),
            error: None,
        }
    }

    pub fn name(&self, entry: &RawEntry) -> &[u8] {
        let start = entry.name_off as usize;
        &self.names[start..start + entry.name_len as usize]
    }

    pub fn push(&mut self, name: &[u8], mut entry: RawEntry) {
        let len = name.len().min(u16::MAX as usize);
        entry.name_off = self.names.len() as u32;
        entry.name_len = len as u16;
        self.names.extend_from_slice(&name[..len]);
        self.entries.push(entry);
    }
}

pub struct ReadContext<'a> {
    pub excludes: &'a Excludes,
}

pub fn read_dir(path: &Path, batch: &mut DirBatch, context: &ReadContext) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("DIRSTAT_PORTABLE_READER").is_none() {
            return super::linux::read_dir(path, batch, context);
        }
    }
    super::portable::read_dir(path, batch, context)
}

pub fn mtime_secs(seconds: i64) -> u32 {
    seconds.clamp(0, u32::MAX as i64) as u32
}
