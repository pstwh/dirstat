use std::fs;
use std::path::Path;

use dirstat_core::export::{read_ncdu, write_csv, write_ncdu};
use dirstat_core::scan::{ScanOptions, scan_blocking, start_rescan};
use dirstat_core::search::{Query, search};
use dirstat_core::{Metric, NodeId, ScanState, Tree};

fn check_invariants(tree: &Tree) {
    for (id, node) in tree.nodes.iter().enumerate() {
        if node.is_dead() || !node.is_dir() {
            continue;
        }
        let mut size = 0;
        let mut files = 0;
        let mut dirs = 1;
        for child in tree.live_children(id as NodeId) {
            let child = tree.node(child);
            assert_eq!(child.parent, id as NodeId);
            size += child.weight(Metric::Apparent);
            files += child.files;
            dirs += child.dirs;
        }
        assert!(node.size >= size, "size of {:?}", tree.path(id as NodeId));
        assert_eq!(node.files, files, "files of {:?}", tree.path(id as NodeId));
        assert_eq!(node.dirs, dirs, "dirs of {:?}", tree.path(id as NodeId));
    }
}

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![7u8; bytes]).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("a/one.txt"), 100);
    write(&root.join("a/two.TXT"), 200);
    write(&root.join("a/b/c/deep.bin"), 5000);
    write(&root.join("photo.jpg"), 3000);
    write(&root.join("node_modules/x/y.js"), 10);
    fs::create_dir_all(root.join("empty")).unwrap();
    dir
}

fn scan(root: &Path) -> Tree {
    let mut options = ScanOptions::new(root);
    options.metric = Metric::Apparent;
    scan_blocking(options).unwrap().0
}

fn file_size_sum(tree: &Tree) -> u64 {
    tree.nodes
        .iter()
        .filter(|n| !n.is_dir() && !n.is_dead())
        .map(|n| n.weight(Metric::Apparent))
        .sum()
}

#[test]
fn scan_counts_files_folders_and_sizes() {
    let dir = fixture();
    let tree = scan(dir.path());
    check_invariants(&tree);
    let root = tree.node(0);
    assert_eq!(root.files, 5);
    assert_eq!(root.folders(), 6);
    assert_eq!(file_size_sum(&tree), 8310);
    let a = tree.find_path(&dir.path().join("a")).unwrap();
    assert!(tree.node(a).size >= 5300);
    assert_eq!(tree.node(a).files, 3);
    let txt = tree.exts.find("txt").unwrap();
    assert_eq!(tree.exts.stats[txt as usize].files, 2);
    assert_eq!(tree.exts.stats[txt as usize].size, 300);
    let order = tree.sorted_children(0, Metric::Apparent, &mut Vec::new()).to_vec();
    let weights: Vec<u64> = order.iter().map(|&c| tree.weight(c, Metric::Apparent)).collect();
    assert!(weights.windows(2).all(|w| w[0] >= w[1]));
}

#[test]
fn excludes_skip_matching_names() {
    let dir = fixture();
    let mut options = ScanOptions::new(dir.path());
    options.excludes = dirstat_core::exclude::Excludes::parse(&["node_modules".into(), "*.jpg".into()]);
    let (tree, _) = scan_blocking(options).unwrap();
    check_invariants(&tree);
    assert_eq!(tree.node(0).files, 3);
    assert!(tree.find_path(&dir.path().join("node_modules")).is_none());
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed_and_hardlinks_count_once() {
    let dir = fixture();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("a/loop")).unwrap();
    fs::hard_link(dir.path().join("a/b/c/deep.bin"), dir.path().join("hard.bin")).unwrap();
    let tree = scan(dir.path());
    check_invariants(&tree);
    let link = tree.find_path(&dir.path().join("a/loop")).unwrap();
    assert!(tree.node(link).flags.has(dirstat_core::NodeFlags::SYMLINK));
    let hardlinks = tree
        .nodes
        .iter()
        .filter(|n| n.flags.has(dirstat_core::NodeFlags::HARDLINK))
        .count();
    assert_eq!(hardlinks, 1);
    assert_eq!(file_size_sum(&tree), 8310 + tree.node(link).size);
}

#[test]
fn deep_and_wide_trees_scan() {
    let dir = tempfile::tempdir().unwrap();
    let mut deep = dir.path().to_path_buf();
    for level in 0..120 {
        deep.push(format!("d{level}"));
    }
    write(&deep.join("leaf"), 1);
    let wide = dir.path().join("wide");
    fs::create_dir_all(&wide).unwrap();
    for index in 0..5000 {
        fs::write(wide.join(format!("f{index}")), b"x").unwrap();
    }
    let tree = scan(dir.path());
    check_invariants(&tree);
    assert_eq!(tree.node(0).files, 5001);
    assert_eq!(tree.node(0).folders(), 121);
}

#[test]
fn rescan_picks_up_changes() {
    let dir = fixture();
    let mut options = ScanOptions::new(dir.path());
    options.metric = Metric::Apparent;
    let (tree, progress) = dirstat_core::start_scan(options.clone());
    while progress.is_running() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(progress.state(), ScanState::Done);
    let a = tree.read().find_path(&dir.path().join("a")).unwrap();
    fs::remove_file(dir.path().join("a/one.txt")).unwrap();
    write(&dir.path().join("a/new.dat"), 4000);
    let job = start_rescan(&tree, a, options);
    while job.is_running() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let tree = tree.read();
    check_invariants(&tree);
    assert_eq!(tree.node(0).files, 5);
    assert_eq!(file_size_sum(&tree), 8310 - 100 + 4000);
    assert!(tree.find_path(&dir.path().join("a/new.dat")).is_some());
    assert!(tree.find_path(&dir.path().join("a/one.txt")).is_none());
    let txt = tree.exts.find("txt").unwrap();
    assert_eq!(tree.exts.stats[txt as usize].files, 1);
}

#[test]
fn search_finds_by_name_extension_and_size() {
    let dir = fixture();
    let tree = scan(dir.path());
    let hits = search(&tree, &Query::parse("ext:txt"), 0, Metric::Apparent, 10);
    assert_eq!(hits.len(), 2);
    assert_eq!(tree.name_lossy(hits[0]), "two.TXT");
    let hits = search(&tree, &Query::parse("DEEP"), 0, Metric::Apparent, 10);
    assert_eq!(hits.len(), 1);
    let hits = search(&tree, &Query::parse("size:>2k kind:file"), 0, Metric::Apparent, 10);
    assert_eq!(hits.len(), 2);
    let largest = dirstat_core::largest::largest_files(&tree, 0, Metric::Apparent, 2);
    assert_eq!(tree.name_lossy(largest[0]), "deep.bin");
    assert_eq!(largest.len(), 2);
}

#[test]
fn exports_round_trip() {
    let dir = fixture();
    let tree = scan(dir.path());
    let mut csv = Vec::new();
    write_csv(&tree, 0, Metric::Apparent, &mut csv).unwrap();
    let text = String::from_utf8(csv).unwrap();
    assert_eq!(text.lines().count(), tree.len() + 1);
    let mut json = Vec::new();
    write_ncdu(&tree, 0, &mut json).unwrap();
    let back = read_ncdu(std::io::Cursor::new(json)).unwrap();
    check_invariants(&back);
    assert_eq!(back.node(0).size, tree.node(0).size);
    assert_eq!(back.node(0).alloc, tree.node(0).alloc);
    assert_eq!(back.node(0).files, tree.node(0).files);
    assert_eq!(back.len(), tree.len());
}

#[test]
fn duplicates_are_grouped_by_content() {
    let dir = tempfile::tempdir().unwrap();
    let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let mut other = big.clone();
    other[50_000] ^= 1;
    fs::write(dir.path().join("a.bin"), &big).unwrap();
    fs::write(dir.path().join("b.bin"), &big).unwrap();
    fs::write(dir.path().join("c.bin"), &other).unwrap();
    fs::write(dir.path().join("s1"), b"same small").unwrap();
    fs::write(dir.path().join("s2"), b"same small").unwrap();
    let tree = scan(dir.path());
    let lock = parking_lot::RwLock::new(tree);
    let progress = dirstat_core::dupes::DupProgress::default();
    let groups = dirstat_core::dupes::find_duplicates(&lock, 1, 2, &progress);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].nodes.len(), 2);
    assert_eq!(groups[0].size, 100_000);
    assert_eq!(groups[1].size, 10);
}

#[cfg(target_os = "linux")]
#[test]
fn portable_and_linux_readers_agree() {
    let dir = fixture();
    let fast = scan(dir.path());
    let mut entries = Vec::new();
    for item in fs::read_dir(dir.path()).unwrap() {
        entries.push(item.unwrap().file_name());
    }
    assert_eq!(fast.node(0).child_count as usize, entries.len());
}
