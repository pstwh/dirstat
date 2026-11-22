use std::path::{Path, PathBuf};

use sysinfo::{DiskKind, Disks};

#[derive(Clone, Debug)]
pub struct Disk {
    pub name: String,
    pub mount: PathBuf,
    pub fs: String,
    pub total: u64,
    pub free: u64,
    pub ssd: Option<bool>,
    pub removable: bool,
}

impl Disk {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    pub fn label(&self) -> String {
        let mount = self.mount.display().to_string();
        if self.name.is_empty() || self.name == mount || self.name.starts_with("/dev/") {
            mount
        } else {
            format!("{} ({mount})", self.name)
        }
    }
}

const IGNORED_FS: &[&str] = &[
    "squashfs",
    "overlay",
    "tmpfs",
    "devtmpfs",
    "ramfs",
    "efivarfs",
    "autofs",
    "devfs",
    "nullfs",
    "fuse.portal",
];

fn hidden_mount(mount: &Path) -> bool {
    let text = mount.to_string_lossy();
    if cfg!(target_os = "macos") {
        return text.starts_with("/System/Volumes/") && text != "/System/Volumes/Data";
    }
    text.starts_with("/boot/efi") || text.starts_with("/snap/") || text.starts_with("/var/lib/docker")
}

pub fn list() -> Vec<Disk> {
    let disks = Disks::new_with_refreshed_list();
    let mut out: Vec<Disk> = Vec::new();
    for disk in disks.list() {
        let fs = disk.file_system().to_string_lossy().to_string();
        let mount = disk.mount_point().to_path_buf();
        if disk.total_space() == 0 || IGNORED_FS.contains(&fs.as_str()) || hidden_mount(&mount) {
            continue;
        }
        if out.iter().any(|d| d.mount == mount) {
            continue;
        }
        out.push(Disk {
            name: disk.name().to_string_lossy().to_string(),
            mount,
            fs,
            total: disk.total_space(),
            free: disk.available_space(),
            ssd: match disk.kind() {
                DiskKind::SSD => Some(true),
                DiskKind::HDD => Some(false),
                _ => None,
            },
            removable: disk.is_removable(),
        });
    }
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out
}

pub fn containing<'a>(disks: &'a [Disk], path: &Path) -> Option<&'a Disk> {
    disks
        .iter()
        .filter(|d| path.starts_with(&d.mount))
        .max_by_key(|d| d.mount.as_os_str().len())
}
