use std::path::Path;

use dirstat_core::cleanup::{RULES, find};
use dirstat_core::paths::clean_path;
use dirstat_core::scan::{ScanOptions, scan_blocking};

fn write(root: &Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![7u8; 5000]).unwrap();
}

#[test]
fn cleanup_skips_app_data_and_installed_packages() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = clean_path(std::fs::canonicalize(dir.path()).unwrap());
    for file in [
        "code/app/package.json",
        "code/app/node_modules/lib/index.js",
        "code/app/src/__pycache__/a.pyc",
        "code/lib/Cargo.toml",
        "code/lib/target/debug/lib.rlib",
        "code/photos/target/a.jpg",
        "code/env/.venv/pyvenv.cfg",
        "code/env/.venv/lib/x.py",
        "npm-global/lib/node_modules/tool/package.json",
        "npm-global/lib/node_modules/tool/node_modules/dep/index.js",
        ".vscode/extensions/ext/package.json",
        ".vscode/extensions/ext/node_modules/dep/index.js",
        ".local/share/flatpak/runtime/lib/__pycache__/b.pyc",
        ".local/share/Trash/files/old.txt",
        ".cache/pip/wheel.whl",
        ".cargo/registry/cache/crate.crate",
        "Library/Caches/app/blob",
    ] {
        write(&home, file);
    }
    let (tree, _) = scan_blocking(ScanOptions::new(&home)).unwrap();
    let mut found: Vec<String> = find(&tree, 0, Some(&home))
        .into_iter()
        .map(|hit| {
            let path = tree.path(hit.node);
            let relative = path.strip_prefix(&home).unwrap().to_string_lossy().replace('\\', "/");
            format!("{} {relative}", RULES[hit.rule].kind)
        })
        .collect();
    found.sort();
    let mut expected = vec![
        "node_modules code/app/node_modules",
        "python_cache code/app/src/__pycache__",
        "rust_target code/lib/target",
        "python_venv code/env/.venv",
        "trash .local/share/Trash",
        "user_cache .cache",
        "package_cache .cargo/registry",
        "macos_caches Library/Caches",
    ];
    expected.sort();
    assert_eq!(found, expected);
    let inside = tree.find_path(&home.join(".vscode")).unwrap();
    assert!(find(&tree, inside, Some(&home)).is_empty());
}
