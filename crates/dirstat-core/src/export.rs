use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::format::now_secs;
use crate::tree::{Metric, NONE, Node, NodeFlags, NodeId, Totals, Tree, push_name};

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn walk(
    tree: &Tree,
    root: NodeId,
    metric: Metric,
    mut visit: impl FnMut(NodeId, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut scratch = Vec::new();
    let mut stack: Vec<(NodeId, PathBuf)> = vec![(root, tree.path(root))];
    while let Some((id, path)) = stack.pop() {
        visit(id, &path)?;
        if tree.node(id).is_dir() {
            let children: Vec<u32> = tree.sorted_children(id, metric, &mut scratch).to_vec();
            for child in children.into_iter().rev() {
                if !tree.node(child).is_dead() {
                    stack.push((child, path.join(tree.name(child))));
                }
            }
        }
    }
    Ok(())
}

pub fn write_csv(tree: &Tree, root: NodeId, metric: Metric, out: &mut impl Write) -> std::io::Result<()> {
    writeln!(out, "path,size,allocated,files,folders,modified,kind")?;
    walk(tree, root, metric, |id, path| {
        let node = tree.node(id);
        let kind = if node.is_dir() {
            "folder"
        } else if node.flags.has(NodeFlags::SYMLINK) {
            "symlink"
        } else if node.flags.has(NodeFlags::HARDLINK) {
            "hardlink"
        } else {
            "file"
        };
        writeln!(
            out,
            "{},{},{},{},{},{},{kind}",
            csv_field(&path.to_string_lossy()),
            node.size,
            node.alloc,
            node.files,
            node.folders(),
            node.mtime
        )
    })
}

fn own_totals(tree: &Tree, id: NodeId) -> (u64, u64) {
    let node = tree.node(id);
    if !node.is_dir() {
        return (node.size, node.alloc);
    }
    let (mut size, mut alloc) = (node.size, node.alloc);
    for child in tree.live_children(id) {
        let child = tree.node(child);
        size = size.saturating_sub(child.weight(Metric::Apparent));
        alloc = alloc.saturating_sub(child.weight(Metric::Allocated));
    }
    (size, alloc)
}

fn info(tree: &Tree, id: NodeId, name: &str) -> Value {
    let node = tree.node(id);
    let (asize, dsize) = own_totals(tree, id);
    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::from(name));
    map.insert("asize".into(), Value::from(asize));
    map.insert("dsize".into(), Value::from(dsize));
    if node.mtime > 0 {
        map.insert("mtime".into(), Value::from(node.mtime));
    }
    if node.flags.has(NodeFlags::ERROR) {
        map.insert("read_error".into(), Value::from(true));
    }
    if node.flags.has(NodeFlags::HARDLINK) {
        map.insert("hlnkc".into(), Value::from(true));
    }
    if node.flags.has(NodeFlags::SYMLINK) || node.flags.has(NodeFlags::OTHER) {
        map.insert("notreg".into(), Value::from(true));
    }
    if node.flags.has(NodeFlags::MOUNT) && node.child_count == 0 {
        map.insert("excluded".into(), Value::from("otherfs"));
    }
    Value::Object(map)
}

fn write_node(tree: &Tree, id: NodeId, name: &str, out: &mut impl Write) -> std::io::Result<()> {
    let header = serde_json::to_string(&info(tree, id, name)).map_err(std::io::Error::other)?;
    if !tree.node(id).is_dir() {
        return out.write_all(header.as_bytes());
    }
    write!(out, "[{header}")?;
    for child in tree.live_children(id) {
        out.write_all(b",\n")?;
        write_node(tree, child, &tree.name_lossy(child), out)?;
    }
    out.write_all(b"]")
}

pub fn write_ncdu(tree: &Tree, root: NodeId, out: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        out,
        "[1,2,{{\"progname\":\"dirstat\",\"progver\":\"{}\",\"timestamp\":{}}},",
        env!("CARGO_PKG_VERSION"),
        now_secs()
    )?;
    let name = tree.path(root).to_string_lossy().to_string();
    write_node(tree, root, &name, out)?;
    out.write_all(b"]\n")
}

fn read_info(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    match value {
        Value::Array(items) => items.first()?.as_object(),
        Value::Object(map) => Some(map),
        _ => None,
    }
}

fn number(map: &serde_json::Map<String, Value>, key: &str) -> u64 {
    map.get(key).and_then(Value::as_u64).unwrap_or(0)
}

pub fn read_ncdu(input: impl BufRead) -> Result<Tree, String> {
    let value: Value = serde_json::from_reader(input).map_err(|e| e.to_string())?;
    let items = value.as_array().ok_or("not an ncdu export")?;
    let root_value = items.get(3).ok_or("the export has no data")?;
    let root_info = read_info(root_value).ok_or("the export has no root folder")?;
    let root_name = root_info.get("name").and_then(Value::as_str).unwrap_or("/");
    let mut tree = Tree::new(Path::new(root_name));
    let root_own = Totals {
        size: number(root_info, "asize"),
        alloc: number(root_info, "dsize"),
        files: 0,
        dirs: 0,
        mtime: number(root_info, "mtime").min(u32::MAX as u64) as u32,
    };
    tree.add_up(0, root_own);
    let mut queue: Vec<(NodeId, &Value)> = vec![(0, root_value)];
    while let Some((parent, value)) = queue.pop() {
        let Value::Array(children) = value else { continue };
        let first = tree.nodes.len() as u32;
        let depth = tree.nodes[parent as usize].depth.saturating_add(1);
        let mut totals = Totals::default();
        let mut count = 0u32;
        for child in children.iter().skip(1) {
            let Some(map) = read_info(child) else { continue };
            let name = map.get("name").and_then(Value::as_str).unwrap_or("");
            let is_dir = child.is_array();
            let mut flags = NodeFlags::default();
            if is_dir {
                flags.set(NodeFlags::DIR);
            }
            if map.get("read_error").and_then(Value::as_bool).unwrap_or(false) {
                flags.set(NodeFlags::ERROR);
            }
            if map.get("hlnkc").and_then(Value::as_bool).unwrap_or(false) && !is_dir {
                flags.set(NodeFlags::HARDLINK);
            }
            if map.get("notreg").and_then(Value::as_bool).unwrap_or(false) {
                flags.set(NodeFlags::OTHER);
            }
            if map.get("excluded").and_then(Value::as_str) == Some("otherfs") {
                flags.set(NodeFlags::MOUNT);
            }
            let size = number(map, "asize");
            let alloc = number(map, "dsize");
            let mtime = number(map, "mtime").min(u32::MAX as u64) as u32;
            let name_off = push_name(&mut tree.names, name.as_bytes());
            let ext = if is_dir {
                0
            } else {
                tree.exts.intern_name(name.as_bytes())
            };
            let node = Node {
                size,
                alloc,
                parent,
                name_off,
                files: if is_dir { 0 } else { 1 },
                dirs: if is_dir { 1 } else { 0 },
                mtime,
                ext,
                flags,
                depth,
                ..Node::default()
            };
            if !is_dir && !flags.has(NodeFlags::HARDLINK) {
                tree.exts.add(ext, size, alloc);
            }
            let own = Totals::of(&node);
            totals.size += own.size;
            totals.alloc += own.alloc;
            totals.files += own.files;
            totals.dirs += own.dirs;
            totals.mtime = totals.mtime.max(mtime);
            let id = tree.nodes.len() as u32;
            tree.nodes.push(node);
            tree.order.push(id);
            if is_dir {
                queue.push((id, child));
            }
            count += 1;
        }
        tree.nodes[parent as usize].first_child = first;
        tree.nodes[parent as usize].child_count = count;
        tree.add_up(parent, totals);
    }
    debug_assert!(tree.nodes[0].parent == NONE);
    tree.sort_all(Metric::Allocated);
    tree.exts.assign_colors(Metric::Allocated);
    Ok(tree)
}
