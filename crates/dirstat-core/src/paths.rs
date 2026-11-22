use std::path::{Path, PathBuf};

pub fn data_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os("DIRSTAT_DATA_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(custom);
    }
    if cfg!(windows)
        && let Some(appdata) = std::env::var_os("APPDATA")
    {
        return PathBuf::from(appdata).join("dirstat");
    }
    let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        return home.join("Library").join("Application Support").join("dirstat");
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("dirstat");
    }
    home.join(".config").join("dirstat")
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
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
