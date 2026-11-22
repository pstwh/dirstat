use std::path::{Path, PathBuf};

use dirstat_core::Metric;
use dirstat_core::exclude::Excludes;
use dirstat_core::paths::{data_dir, write_atomic};
use dirstat_core::scan::ScanOptions;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Arrangement {
    #[default]
    SideBySide,
    Stacked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub appearance: Appearance,
    pub metric: Metric,
    pub cross_filesystems: bool,
    pub dedup_hardlinks: bool,
    pub threads: usize,
    pub excludes: Vec<String>,
    pub cushion: bool,
    pub brightness: f32,
    pub cushion_height: f32,
    pub arrangement: Arrangement,
    pub show_treemap: bool,
    pub show_sidebar: bool,
    pub recent: Vec<PathBuf>,
    pub dupes_min_size: u64,
    pub confirm_trash: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            appearance: Appearance::System,
            metric: Metric::Allocated,
            cross_filesystems: cfg!(target_os = "macos"),
            dedup_hardlinks: true,
            threads: 0,
            excludes: Vec::new(),
            cushion: true,
            brightness: 1.0,
            cushion_height: 0.38,
            arrangement: Arrangement::SideBySide,
            show_treemap: true,
            show_sidebar: true,
            recent: Vec::new(),
            dupes_min_size: 1024 * 1024,
            confirm_trash: true,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        data_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        std::fs::read(Settings::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = write_atomic(&Settings::path(), &bytes);
        }
    }

    pub fn scan_options(&self, root: &Path) -> ScanOptions {
        let mut options = ScanOptions::new(root);
        options.cross_filesystems = self.cross_filesystems;
        options.dedup_hardlinks = self.dedup_hardlinks;
        options.threads = self.threads;
        options.excludes = Excludes::parse(&self.excludes);
        options.metric = self.metric;
        options
    }

    pub fn remember(&mut self, root: &Path) {
        self.recent.retain(|p| p != root);
        self.recent.insert(0, root.to_path_buf());
        self.recent.truncate(8);
    }
}
