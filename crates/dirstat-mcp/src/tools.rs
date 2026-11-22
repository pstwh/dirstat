use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dirstat_core::cleanup::{self, RULES};
use dirstat_core::disks;
use dirstat_core::dupes::{DupProgress, find_duplicates_within};
use dirstat_core::exclude::Excludes;
use dirstat_core::format::{human_count, human_date, human_duration, human_size};
use dirstat_core::largest::largest_files;
use dirstat_core::paths::{clean_path, home_dir};
use dirstat_core::scan::{ScanOptions, scan_blocking};
use dirstat_core::search::{Query, search};
use dirstat_core::{Metric, NodeFlags, NodeId, SharedTree, Tree};
use parking_lot::RwLock;
use serde_json::{Value, json};

const REUSE_FOR: Duration = Duration::from_secs(600);
const KEEP_SCANS: usize = 3;

#[derive(Clone, Debug)]
pub struct Options {
    pub excludes: Vec<String>,
    pub cross_filesystems: bool,
    pub dedup_hardlinks: bool,
    pub threads: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            excludes: Vec::new(),
            cross_filesystems: false,
            dedup_hardlinks: true,
            threads: 0,
        }
    }
}

pub enum Outcome {
    Unknown,
    Done(Result<Value, String>),
}

struct Scan {
    root: PathBuf,
    tree: SharedTree,
    finished: Instant,
    took: Duration,
}

struct Spec {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    schema: fn() -> Value,
}

pub struct Tools {
    options: Options,
    scans: Vec<Scan>,
}

fn path_property() -> Value {
    json!({ "type": "string", "description": "Folder or drive, absolute or starting with ~, for example / or C:\\ or ~/Downloads" })
}

fn metric_property() -> Value {
    json!({ "type": "string", "enum": ["on_disk", "apparent"], "description": "on_disk (default) counts allocated space, apparent counts file lengths" })
}

fn limit_property(default: u64, max: u64) -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": max, "description": format!("How many to return, default {default}, at most {max}") })
}

fn specs() -> Vec<Spec> {
    vec![
        Spec {
            name: "disks",
            title: "Disks",
            description: "List mounted drives with their file system, total, used and free space.",
            schema: || json!({ "type": "object", "properties": {} }),
        },
        Spec {
            name: "scan",
            title: "Scan a folder",
            description: "Measure every file under a folder or drive and keep the result for the other tools. Returns totals, the biggest items at the top level and the biggest file types. Rescans even when a recent scan exists.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "path": path_property(),
                        "cross_filesystems": { "type": "boolean", "description": "Also enter other mounted drives below the path, default false" },
                    },
                    "required": ["path"],
                })
            },
        },
        Spec {
            name: "top",
            title: "Biggest folders",
            description: "The biggest items inside a folder, sorted by size, optionally nested a few levels deep.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "path": path_property(),
                        "depth": { "type": "integer", "minimum": 1, "maximum": 4, "description": "Levels to expand, default 1" },
                        "limit": limit_property(15, 100),
                        "metric": metric_property(),
                    },
                    "required": ["path"],
                })
            },
        },
        Spec {
            name: "largest_files",
            title: "Largest files",
            description: "The largest files anywhere under a folder.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "path": path_property(),
                        "limit": limit_property(30, 500),
                        "metric": metric_property(),
                    },
                    "required": ["path"],
                })
            },
        },
        Spec {
            name: "types",
            title: "Space by file type",
            description: "Total size and file count per file extension under a folder.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "path": path_property(),
                        "limit": limit_property(25, 200),
                        "metric": metric_property(),
                    },
                    "required": ["path"],
                })
            },
        },
        Spec {
            name: "search",
            title: "Search files",
            description: "Find files and folders under a folder by name words and filters: ext:pdf,docx  size:>100mb  size:<1kb  older:1y  newer:7d  kind:file or kind:dir. Results are sorted by size.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "path": path_property(),
                        "query": { "type": "string", "description": "For example: invoice ext:pdf older:2y" },
                        "limit": limit_property(50, 1000),
                        "metric": metric_property(),
                    },
                    "required": ["path", "query"],
                })
            },
        },
        Spec {
            name: "duplicates",
            title: "Duplicate files",
            description: "Files with identical content under a folder, matched by size, a sample and a full BLAKE3 hash. Groups are sorted by the space the extra copies waste. Reads the candidate files, so it can take a while on big folders.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "path": path_property(),
                        "min_size": { "type": "string", "description": "Ignore files smaller than this, default 1mb" },
                        "limit": limit_property(20, 200),
                    },
                    "required": ["path"],
                })
            },
        },
        Spec {
            name: "cleanup_candidates",
            title: "Cleanup candidates",
            description: "Folders under a path that can usually be removed and recreated: node_modules, Rust target, Python caches and virtual environments, web build caches, Gradle, Xcode DerivedData, Terraform, user and package caches, temp folders and the trash. Grouped by kind with the reason it is safe and the biggest locations.",
            schema: || {
                json!({
                    "type": "object",
                    "properties": {
                        "path": path_property(),
                        "limit": limit_property(10, 100),
                    },
                    "required": ["path"],
                })
            },
        },
    ]
}

fn string(arguments: &Value, key: &str) -> Option<String> {
    arguments.get(key).and_then(Value::as_str).map(str::to_string)
}

fn limit(arguments: &Value, default: usize, max: usize) -> usize {
    arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(default)
        .clamp(1, max)
}

fn metric(arguments: &Value) -> Result<Metric, String> {
    match arguments.get("metric").and_then(Value::as_str) {
        None | Some("on_disk") => Ok(Metric::Allocated),
        Some("apparent") => Ok(Metric::Apparent),
        Some(other) => Err(format!("metric must be on_disk or apparent, not {other}")),
    }
}

fn resolve(raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("path is empty".into());
    }
    let expanded = match raw.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => {
            let home = home_dir().ok_or("the home folder is unknown")?;
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(raw),
    };
    if !expanded.is_absolute() {
        return Err(format!("{raw} is not an absolute path"));
    }
    let canonical = std::fs::canonicalize(&expanded).map_err(|error| format!("{}: {error}", expanded.display()))?;
    Ok(clean_path(canonical))
}

fn kind(tree: &Tree, id: NodeId) -> &'static str {
    let node = tree.node(id);
    if node.flags.has(NodeFlags::SYMLINK) {
        "symlink"
    } else if node.flags.has(NodeFlags::MOUNT) {
        "mount"
    } else if node.is_dir() {
        "dir"
    } else {
        "file"
    }
}

fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".into();
    }
    format!("{:.1}%", part as f64 / whole as f64 * 100.0)
}

fn path_text(tree: &Tree, id: NodeId) -> String {
    tree.path(id).display().to_string()
}

fn sorted(tree: &Tree, id: NodeId, metric: Metric) -> Vec<NodeId> {
    let mut scratch = Vec::new();
    tree.sorted_children(id, metric, &mut scratch)
        .iter()
        .copied()
        .filter(|&child| !tree.node(child).is_dead())
        .collect()
}

fn item(tree: &Tree, id: NodeId, metric: Metric, depth: u32, limit: usize, whole: u64) -> Value {
    let node = tree.node(id);
    let size = node.weight(metric);
    let mut entry = json!({
        "name": tree.display_name(id),
        "kind": kind(tree, id),
        "size": human_size(size),
        "share": percent(size, whole),
    });
    let modified = human_date(node.mtime);
    if !modified.is_empty() {
        entry["modified"] = json!(modified);
    }
    if node.flags.has(NodeFlags::MOUNT) {
        entry["note"] = json!("another drive, not scanned");
    }
    if node.is_dir() && !node.flags.has(NodeFlags::MOUNT) {
        entry["files"] = json!(node.files);
        if depth > 0 && node.child_count > 0 {
            entry["children"] = children(tree, id, metric, depth - 1, limit);
        }
    }
    entry
}

fn children(tree: &Tree, id: NodeId, metric: Metric, depth: u32, limit: usize) -> Value {
    let whole = tree.weight(id, metric);
    let list = sorted(tree, id, metric);
    let mut out: Vec<Value> = list
        .iter()
        .take(limit)
        .map(|&child| item(tree, child, metric, depth, limit, whole))
        .collect();
    if list.len() > limit {
        let rest = &list[limit..];
        let size: u64 = rest.iter().map(|&child| tree.weight(child, metric)).sum();
        out.push(json!({
            "name": format!("{} more items", rest.len()),
            "kind": "others",
            "size": human_size(size),
            "share": percent(size, whole),
        }));
    }
    Value::Array(out)
}

fn ext_totals(tree: &Tree, within: NodeId) -> HashMap<u16, (u64, u64, u64)> {
    let mut totals: HashMap<u16, (u64, u64, u64)> = HashMap::new();
    let mut stack = vec![within];
    while let Some(node) = stack.pop() {
        for child in tree.live_children(node) {
            let entry = tree.node(child);
            if entry.is_dir() {
                stack.push(child);
            } else if !entry.flags.has(NodeFlags::HARDLINK) {
                let slot = totals.entry(entry.ext).or_default();
                slot.0 += 1;
                slot.1 += entry.size;
                slot.2 += entry.alloc;
            }
        }
    }
    totals
}

fn types_of(tree: &Tree, within: NodeId, metric: Metric, limit: usize) -> Value {
    let totals: Vec<(u16, u64, u64)> = if within == tree.root {
        tree.exts
            .stats
            .iter()
            .enumerate()
            .filter(|(_, stat)| stat.files > 0)
            .map(|(id, stat)| (id as u16, stat.files, stat.weight(metric)))
            .collect()
    } else {
        ext_totals(tree, within)
            .into_iter()
            .map(|(id, (files, size, alloc))| {
                let weight = match metric {
                    Metric::Allocated => alloc,
                    Metric::Apparent => size,
                };
                (id, files, weight)
            })
            .collect()
    };
    let mut totals = totals;
    totals.sort_unstable_by_key(|&(_, _, weight)| std::cmp::Reverse(weight));
    let whole: u64 = totals.iter().map(|&(_, _, weight)| weight).sum();
    let list: Vec<Value> = totals
        .iter()
        .take(limit)
        .map(|&(id, files, weight)| {
            let name = &tree.exts.stats[id as usize].name;
            json!({
                "extension": if name.is_empty() { "(none)".to_string() } else { format!(".{name}") },
                "files": files,
                "size": human_size(weight),
                "share": percent(weight, whole),
            })
        })
        .collect();
    json!({ "kinds": totals.len(), "types": list })
}

fn file_entry(tree: &Tree, id: NodeId, metric: Metric) -> Value {
    let node = tree.node(id);
    let mut entry = json!({
        "path": path_text(tree, id),
        "size": human_size(node.weight(metric)),
    });
    if node.is_dir() {
        entry["kind"] = json!("dir");
    }
    let modified = human_date(node.mtime);
    if !modified.is_empty() {
        entry["modified"] = json!(modified);
    }
    entry
}

fn ago(instant: Instant) -> String {
    let seconds = instant.elapsed().as_secs();
    match seconds {
        0..=59 => format!("{seconds} s ago"),
        60..=3599 => format!("{} min ago", seconds / 60),
        _ => format!("{} h ago", seconds / 3600),
    }
}

impl Tools {
    pub fn new(options: Options) -> Tools {
        Tools {
            options,
            scans: Vec::new(),
        }
    }

    pub fn definitions(&self) -> Vec<Value> {
        specs()
            .iter()
            .map(|spec| {
                json!({
                    "name": spec.name,
                    "title": spec.title,
                    "description": spec.description,
                    "inputSchema": (spec.schema)(),
                    "annotations": {
                        "title": spec.title,
                        "readOnlyHint": true,
                        "destructiveHint": false,
                        "idempotentHint": true,
                        "openWorldHint": false,
                    },
                })
            })
            .collect()
    }

    pub fn call(&mut self, name: &str, arguments: &Value) -> Outcome {
        let result = match name {
            "disks" => Ok(self.disks()),
            "scan" => self.scan_tool(arguments),
            "top" => self.top(arguments),
            "largest_files" => self.largest(arguments),
            "types" => self.types(arguments),
            "search" => self.search(arguments),
            "duplicates" => self.duplicates(arguments),
            "cleanup_candidates" => self.cleanup(arguments),
            _ => return Outcome::Unknown,
        };
        Outcome::Done(result)
    }

    fn disks(&self) -> Value {
        let list: Vec<Value> = disks::list()
            .iter()
            .map(|disk| {
                json!({
                    "mount": disk.mount.display().to_string(),
                    "name": disk.name,
                    "file_system": disk.fs,
                    "total": human_size(disk.total),
                    "used": human_size(disk.used()),
                    "free": human_size(disk.free),
                    "used_share": percent(disk.used(), disk.total),
                    "removable": disk.removable,
                })
            })
            .collect();
        json!({ "disks": list })
    }

    fn scan_options(&self, root: &Path, cross: bool) -> ScanOptions {
        let mut options = ScanOptions::new(root);
        options.cross_filesystems = cross || self.options.cross_filesystems;
        options.dedup_hardlinks = self.options.dedup_hardlinks;
        options.threads = self.options.threads;
        options.excludes = Excludes::parse(&self.options.excludes);
        options
    }

    fn run_scan(&mut self, root: &Path, cross: bool) -> Result<usize, String> {
        let started = Instant::now();
        let (tree, _) = scan_blocking(self.scan_options(root, cross))?;
        let scan = Scan {
            root: root.to_path_buf(),
            tree: Arc::new(RwLock::new(tree)),
            finished: Instant::now(),
            took: started.elapsed(),
        };
        self.scans.retain(|old| !old.root.starts_with(root));
        self.scans.push(scan);
        while self.scans.len() > KEEP_SCANS {
            self.scans.remove(0);
        }
        Ok(self.scans.len() - 1)
    }

    fn covering(&self, path: &Path) -> Option<(usize, NodeId)> {
        self.scans
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, scan)| scan.finished.elapsed() < REUSE_FOR && path.starts_with(&scan.root))
            .find_map(|(index, scan)| scan.tree.read().find_path(path).map(|node| (index, node)))
    }

    fn target(&mut self, arguments: &Value) -> Result<(usize, NodeId), String> {
        let raw = string(arguments, "path").ok_or("path is required")?;
        let path = resolve(&raw)?;
        if let Some(found) = self.covering(&path) {
            return Ok(found);
        }
        let index = self.run_scan(&path, false)?;
        Ok((index, 0))
    }

    fn header(&self, index: usize, node: NodeId) -> Value {
        let scan = &self.scans[index];
        let tree = scan.tree.read();
        json!({
            "path": path_text(&tree, node),
            "scan": {
                "root": scan.root.display().to_string(),
                "finished": ago(scan.finished),
                "took": human_duration(scan.took),
            },
        })
    }

    fn scan_tool(&mut self, arguments: &Value) -> Result<Value, String> {
        let raw = string(arguments, "path").ok_or("path is required")?;
        let path = resolve(&raw)?;
        let cross = arguments
            .get("cross_filesystems")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let index = self.run_scan(&path, cross)?;
        let mut out = self.header(index, 0);
        let scan = &self.scans[index];
        let tree = scan.tree.read();
        let root = tree.node(0);
        out["files"] = json!(human_count(root.files as u64));
        out["folders"] = json!(human_count(root.folders() as u64));
        out["size_on_disk"] = json!(human_size(root.alloc));
        out["apparent_size"] = json!(human_size(root.size));
        if !tree.errors.is_empty() {
            out["unreadable_folders"] = json!(tree.errors.len());
        }
        if let Some(disk) = disks::containing(&disks::list(), &scan.root) {
            out["drive"] = json!({
                "mount": disk.mount.display().to_string(),
                "total": human_size(disk.total),
                "free": human_size(disk.free),
            });
        }
        out["top"] = children(&tree, 0, Metric::Allocated, 0, 12);
        out["types"] = types_of(&tree, 0, Metric::Allocated, 10)["types"].clone();
        Ok(out)
    }

    fn top(&mut self, arguments: &Value) -> Result<Value, String> {
        let metric = metric(arguments)?;
        let depth = arguments.get("depth").and_then(Value::as_u64).unwrap_or(1).clamp(1, 4) as u32;
        let limit = limit(arguments, 15, 100);
        let (index, node) = self.target(arguments)?;
        let mut out = self.header(index, node);
        let tree = self.scans[index].tree.read();
        let entry = tree.node(node);
        out["size"] = json!(human_size(entry.weight(metric)));
        if entry.is_dir() {
            out["files"] = json!(entry.files);
            out["children"] = children(&tree, node, metric, depth - 1, limit);
        }
        Ok(out)
    }

    fn largest(&mut self, arguments: &Value) -> Result<Value, String> {
        let metric = metric(arguments)?;
        let limit = limit(arguments, 30, 500);
        let (index, node) = self.target(arguments)?;
        let mut out = self.header(index, node);
        let tree = self.scans[index].tree.read();
        let files: Vec<Value> = largest_files(&tree, node, metric, limit)
            .into_iter()
            .map(|id| file_entry(&tree, id, metric))
            .collect();
        out["files"] = Value::Array(files);
        Ok(out)
    }

    fn types(&mut self, arguments: &Value) -> Result<Value, String> {
        let metric = metric(arguments)?;
        let limit = limit(arguments, 25, 200);
        let (index, node) = self.target(arguments)?;
        let mut out = self.header(index, node);
        let tree = self.scans[index].tree.read();
        let types = types_of(&tree, node, metric, limit);
        out["kinds"] = types["kinds"].clone();
        out["types"] = types["types"].clone();
        Ok(out)
    }

    fn search(&mut self, arguments: &Value) -> Result<Value, String> {
        let metric = metric(arguments)?;
        let limit = limit(arguments, 50, 1000);
        let text = string(arguments, "query").ok_or("query is required")?;
        let query = Query::parse(&text);
        if query.is_empty() {
            return Err("query is empty".into());
        }
        let (index, node) = self.target(arguments)?;
        let mut out = self.header(index, node);
        let tree = self.scans[index].tree.read();
        let hits = search(&tree, &query, node, metric, limit);
        let total: u64 = hits.iter().map(|&id| tree.weight(id, metric)).sum();
        out["query"] = json!(text);
        out["shown"] = json!(hits.len());
        if hits.len() == limit {
            out["note"] = json!("more matches exist, raise limit or narrow the query");
        }
        out["total_size_shown"] = json!(human_size(total));
        out["results"] = Value::Array(hits.into_iter().map(|id| file_entry(&tree, id, metric)).collect());
        Ok(out)
    }

    fn duplicates(&mut self, arguments: &Value) -> Result<Value, String> {
        let limit = limit(arguments, 20, 200);
        let min_size = match string(arguments, "min_size") {
            Some(text) => dirstat_core::format::parse_size(&text).ok_or(format!("could not read min_size {text}"))?,
            None => 1 << 20,
        };
        let (index, node) = self.target(arguments)?;
        let mut out = self.header(index, node);
        let tree = self.scans[index].tree.clone();
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 8);
        let started = Instant::now();
        let mut groups = find_duplicates_within(&tree, node, min_size, threads, &DupProgress::default());
        groups.sort_by_key(|group| std::cmp::Reverse(group.wasted()));
        let wasted: u64 = groups.iter().map(|group| group.wasted()).sum();
        out["groups"] = json!(groups.len());
        out["wasted"] = json!(human_size(wasted));
        out["took"] = json!(human_duration(started.elapsed()));
        out["min_size"] = json!(human_size(min_size));
        let shown: Vec<Value> = groups
            .iter()
            .take(limit)
            .map(|group| {
                let paths: Vec<String> = group.paths.iter().take(12).map(|p| p.display().to_string()).collect();
                let mut entry = json!({
                    "size_each": human_size(group.size),
                    "copies": group.nodes.len(),
                    "wasted": human_size(group.wasted()),
                    "paths": paths,
                });
                if group.paths.len() > 12 {
                    entry["more_paths"] = json!(group.paths.len() - 12);
                }
                entry
            })
            .collect();
        out["results"] = Value::Array(shown);
        Ok(out)
    }

    fn cleanup(&mut self, arguments: &Value) -> Result<Value, String> {
        let limit = limit(arguments, 10, 100);
        let (index, node) = self.target(arguments)?;
        let mut out = self.header(index, node);
        let tree = self.scans[index].tree.read();
        let home = home_dir().map(|home| std::fs::canonicalize(&home).map(clean_path).unwrap_or(home));
        let hits = cleanup::find(&tree, node, home.as_deref());
        out["note"] = json!(
            "System folders managed by the package manager or the OS are left out. Check that a project is not in use before removing its build output."
        );
        let mut by_rule: Vec<Vec<NodeId>> = vec![Vec::new(); RULES.len()];
        for hit in hits {
            by_rule[hit.rule].push(hit.node);
        }
        let mut groups: Vec<(usize, u64, Vec<NodeId>)> = by_rule
            .into_iter()
            .enumerate()
            .filter(|(_, nodes)| !nodes.is_empty())
            .map(|(rule, mut nodes)| {
                nodes.sort_unstable_by_key(|&id| std::cmp::Reverse(tree.weight(id, Metric::Allocated)));
                let total = nodes.iter().map(|&id| tree.weight(id, Metric::Allocated)).sum();
                (rule, total, nodes)
            })
            .collect();
        groups.sort_unstable_by_key(|&(_, total, _)| std::cmp::Reverse(total));
        let total: u64 = groups.iter().map(|&(_, size, _)| size).sum();
        out["total"] = json!(human_size(total));
        out["candidates"] = Value::Array(
            groups
                .iter()
                .map(|(rule, size, nodes)| {
                    let rule = &RULES[*rule];
                    json!({
                        "kind": rule.kind,
                        "why": rule.why,
                        "folders": nodes.len(),
                        "size": human_size(*size),
                        "biggest": nodes.iter().take(limit).map(|&id| file_entry(&tree, id, Metric::Allocated)).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        );
        Ok(out)
    }
}
