use std::path::{Path, PathBuf};
use std::process::Command;

pub fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

pub fn trash(paths: &[PathBuf]) -> Result<(), String> {
    trash::delete_all(paths).map_err(|e| e.to_string())
}

pub fn delete(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let result = if metadata.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path).or_else(|error| {
            if cfg!(windows) && metadata.file_type().is_symlink() {
                std::fs::remove_dir(path)
            } else {
                Err(error)
            }
        })
    };
    result.map_err(|e| e.to_string())
}

pub fn open_path(path: &Path) -> Result<(), String> {
    open::that_detached(path).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => url.push(*byte as char),
            other => url.push_str(&format!("%{other:02X}")),
        }
    }
    url
}

#[cfg(windows)]
pub fn reveal(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    Command::new("explorer")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn reveal(path: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        return Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string());
    }
    let shown = Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{}", file_url(path)))
        .arg("string:")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if shown {
        return Ok(());
    }
    let target = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    open_path(target)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn file_urls_escape_special_characters() {
        assert_eq!(file_url(Path::new("/a b/c#d")), "file:///a%20b/c%23d");
    }
}
