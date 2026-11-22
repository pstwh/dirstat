use std::borrow::Cow;
use std::cmp::Reverse;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ext::ExtTable;

pub type NodeId = u32;
pub const NONE: NodeId = u32::MAX;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NodeFlags(pub u8);

impl NodeFlags {
    pub const DIR: u8 = 1;
    pub const SYMLINK: u8 = 2;
    pub const OTHER: u8 = 4;
    pub const ERROR: u8 = 8;
    pub const MOUNT: u8 = 16;
    pub const HARDLINK: u8 = 32;
    pub const PSEUDO: u8 = 64;
    pub const DEAD: u8 = 128;

    pub fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    pub fn set(&mut self, bit: u8) {
        self.0 |= bit;
    }

    pub fn clear(&mut self, bit: u8) {
        self.0 &= !bit;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Node {
    pub size: u64,
    pub alloc: u64,
    pub parent: NodeId,
    pub name_off: u32,
    pub first_child: u32,
    pub child_count: u32,
    pub files: u32,
    pub dirs: u32,
    pub mtime: u32,
    pub ext: u16,
    pub flags: NodeFlags,
    pub depth: u8,
}

const _: () = assert!(std::mem::size_of::<Node>() == 48);

impl Node {
    pub fn is_dir(&self) -> bool {
        self.flags.has(NodeFlags::DIR)
    }

    pub fn is_dead(&self) -> bool {
        self.flags.has(NodeFlags::DEAD)
    }

    pub fn children(&self) -> Range<u32> {
        self.first_child..self.first_child + self.child_count
    }

    pub fn weight(&self, metric: Metric) -> u64 {
        if self.flags.has(NodeFlags::HARDLINK) {
            return 0;
        }
        match metric {
            Metric::Allocated => self.alloc,
            Metric::Apparent => self.size,
        }
    }

    pub fn items(&self) -> u64 {
        (self.files as u64 + self.dirs as u64).saturating_sub(1)
    }

    pub fn folders(&self) -> u32 {
        if self.is_dir() { self.dirs.saturating_sub(1) } else { 0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Metric {
    #[default]
    Allocated,
    Apparent,
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::Allocated => "On disk",
            Metric::Apparent => "Apparent",
        }
    }

    pub fn other(self) -> Metric {
        match self {
            Metric::Allocated => Metric::Apparent,
            Metric::Apparent => Metric::Allocated,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Totals {
    pub size: u64,
    pub alloc: u64,
    pub files: u32,
    pub dirs: u32,
    pub mtime: u32,
}

impl Totals {
    pub fn of(node: &Node) -> Totals {
        Totals {
            size: node.weight(Metric::Apparent),
            alloc: node.weight(Metric::Allocated),
            files: node.files,
            dirs: node.dirs,
            mtime: node.mtime,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub order: Vec<u32>,
    pub names: Vec<u8>,
    pub exts: ExtTable,
    pub root: NodeId,
    pub root_path: PathBuf,
    pub generation: u64,
    pub sorted: Option<Metric>,
    pub errors: Vec<(NodeId, ErrorKind)>,
    pub dead: u64,
}

pub fn push_name(names: &mut Vec<u8>, bytes: &[u8]) -> u32 {
    let offset = names.len() as u32;
    let len = bytes.len().min(u16::MAX as usize);
    if len < 255 {
        names.push(len as u8);
    } else {
        names.push(255);
        names.extend_from_slice(&(len as u16).to_le_bytes());
    }
    names.extend_from_slice(&bytes[..len]);
    offset
}

pub fn read_name(names: &[u8], offset: u32) -> &[u8] {
    let offset = offset as usize;
    let Some(&first) = names.get(offset) else {
        return &[];
    };
    if first < 255 {
        &names[offset + 1..offset + 1 + first as usize]
    } else {
        let len = u16::from_le_bytes([names[offset + 1], names[offset + 2]]) as usize;
        &names[offset + 3..offset + 3 + len]
    }
}

impl Tree {
    pub fn new(root_path: &Path) -> Tree {
        let mut names = Vec::with_capacity(1 << 16);
        let name_off = push_name(&mut names, root_path.as_os_str().as_encoded_bytes());
        let root = Node {
            parent: NONE,
            name_off,
            dirs: 1,
            flags: NodeFlags(NodeFlags::DIR),
            ..Node::default()
        };
        Tree {
            nodes: vec![root],
            order: vec![0],
            names,
            exts: ExtTable::new(),
            root: 0,
            root_path: root_path.to_path_buf(),
            generation: 0,
            sorted: None,
            errors: Vec::new(),
            dead: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize).filter(|n| !n.is_dead())
    }

    pub fn name_bytes(&self, id: NodeId) -> &[u8] {
        read_name(&self.names, self.nodes[id as usize].name_off)
    }

    pub fn name(&self, id: NodeId) -> &OsStr {
        unsafe { OsStr::from_encoded_bytes_unchecked(self.name_bytes(id)) }
    }

    pub fn name_lossy(&self, id: NodeId) -> Cow<'_, str> {
        String::from_utf8_lossy(self.name_bytes(id))
    }

    pub fn display_name(&self, id: NodeId) -> String {
        if id == self.root {
            self.root_path.display().to_string()
        } else {
            self.name_lossy(id).into_owned()
        }
    }

    pub fn path(&self, id: NodeId) -> PathBuf {
        let chain = self.ancestors(id);
        let mut path = self.root_path.clone();
        for &node in chain.iter().skip(1) {
            path.push(self.name(node));
        }
        path
    }

    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = Vec::with_capacity(16);
        let mut current = id;
        while current != NONE {
            chain.push(current);
            current = self.nodes[current as usize].parent;
        }
        chain.reverse();
        chain
    }

    pub fn is_ancestor(&self, ancestor: NodeId, id: NodeId) -> bool {
        let mut current = id;
        while current != NONE {
            if current == ancestor {
                return true;
            }
            current = self.nodes[current as usize].parent;
        }
        false
    }

    pub fn weight(&self, id: NodeId, metric: Metric) -> u64 {
        self.nodes[id as usize].weight(metric)
    }

    pub fn live_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes[id as usize]
            .children()
            .filter(move |&child| !self.nodes[child as usize].is_dead())
    }

    pub fn sorted_children<'a>(&'a self, id: NodeId, metric: Metric, scratch: &'a mut Vec<u32>) -> &'a [u32] {
        let range = self.nodes[id as usize].children();
        let block = &self.order[range.start as usize..range.end as usize];
        if self.sorted == Some(metric) {
            return block;
        }
        scratch.clear();
        scratch.extend_from_slice(block);
        let nodes = &self.nodes;
        scratch.sort_unstable_by_key(|&child| Reverse(nodes[child as usize].weight(metric)));
        scratch
    }

    pub fn find_path(&self, path: &Path) -> Option<NodeId> {
        let relative = path.strip_prefix(&self.root_path).ok()?;
        let mut current = self.root;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            let wanted = name.as_encoded_bytes();
            current = self
                .live_children(current)
                .find(|&child| self.name_bytes(child) == wanted)?;
        }
        Some(current)
    }

    pub fn add_up(&mut self, start: NodeId, totals: Totals) {
        let mut current = start;
        while current != NONE {
            let node = &mut self.nodes[current as usize];
            node.size += totals.size;
            node.alloc += totals.alloc;
            node.files += totals.files;
            node.dirs += totals.dirs;
            node.mtime = node.mtime.max(totals.mtime);
            current = node.parent;
        }
    }

    pub fn sub_up(&mut self, start: NodeId, totals: Totals) {
        let mut current = start;
        while current != NONE {
            let node = &mut self.nodes[current as usize];
            node.size = node.size.saturating_sub(totals.size);
            node.alloc = node.alloc.saturating_sub(totals.alloc);
            node.files = node.files.saturating_sub(totals.files);
            node.dirs = node.dirs.saturating_sub(totals.dirs);
            current = node.parent;
        }
    }

    fn kill_descendants(&mut self, id: NodeId) {
        let mut stack: Vec<NodeId> = self.nodes[id as usize].children().collect();
        while let Some(current) = stack.pop() {
            let node = self.nodes[current as usize];
            if node.is_dead() {
                continue;
            }
            if node.is_dir() {
                stack.extend(node.children());
            } else if !node.flags.has(NodeFlags::HARDLINK) {
                self.exts.sub(node.ext, node.size, node.alloc);
            }
            self.nodes[current as usize].flags.set(NodeFlags::DEAD);
            self.dead += 1;
        }
    }

    pub fn remove(&mut self, id: NodeId) {
        if id == self.root || self.nodes[id as usize].is_dead() {
            return;
        }
        let node = self.nodes[id as usize];
        let parent = node.parent;
        self.sub_up(parent, Totals::of(&node));
        self.kill_descendants(id);
        if !node.is_dir() && !node.flags.has(NodeFlags::HARDLINK) {
            self.exts.sub(node.ext, node.size, node.alloc);
        }
        let entry = &mut self.nodes[id as usize];
        entry.flags.set(NodeFlags::DEAD);
        entry.size = 0;
        entry.alloc = 0;
        self.dead += 1;
        self.generation += 1;
        let nodes = &self.nodes;
        self.errors.retain(|(node, _)| !nodes[*node as usize].is_dead());
        self.resort_ancestors(parent);
    }

    pub fn reset_dir(&mut self, id: NodeId, own: Totals) {
        let node = self.nodes[id as usize];
        let parent = node.parent;
        if parent != NONE {
            self.sub_up(parent, Totals::of(&node));
        }
        self.kill_descendants(id);
        let entry = &mut self.nodes[id as usize];
        entry.size = own.size;
        entry.alloc = own.alloc;
        entry.files = 0;
        entry.dirs = 1;
        entry.mtime = own.mtime;
        entry.first_child = 0;
        entry.child_count = 0;
        entry.flags.clear(NodeFlags::ERROR);
        if parent != NONE {
            self.add_up(parent, Totals { dirs: 1, ..own });
        }
        let nodes = &self.nodes;
        self.errors
            .retain(|(node, _)| *node != id && !nodes[*node as usize].is_dead());
        self.generation += 1;
    }

    pub fn resort_ancestors(&mut self, start: NodeId) {
        let Some(metric) = self.sorted else { return };
        let mut current = start;
        while current != NONE {
            self.sort_block(current, metric);
            current = self.nodes[current as usize].parent;
        }
    }

    pub fn sort_block(&mut self, id: NodeId, metric: Metric) {
        let range = self.nodes[id as usize].children();
        let nodes = &self.nodes;
        self.order[range.start as usize..range.end as usize]
            .sort_unstable_by_key(|&child| Reverse(nodes[child as usize].weight(metric)));
    }

    pub fn sort_all(&mut self, metric: Metric) {
        let mut blocks: Vec<(u32, u32)> = self
            .nodes
            .iter()
            .filter(|n| n.is_dir() && n.child_count > 1 && !n.is_dead())
            .map(|n| (n.first_child, n.child_count))
            .collect();
        blocks.sort_unstable();
        let mut slices = Vec::with_capacity(blocks.len());
        let mut rest: &mut [u32] = &mut self.order;
        let mut offset = 0u32;
        for (first, count) in blocks {
            let tail = std::mem::take(&mut rest);
            let (_, tail) = tail.split_at_mut((first - offset) as usize);
            let (block, tail) = tail.split_at_mut(count as usize);
            slices.push(block);
            rest = tail;
            offset = first + count;
        }
        let nodes = &self.nodes;
        slices.into_par_iter().for_each(|block| {
            block.sort_unstable_by_key(|&child| Reverse(nodes[child as usize].weight(metric)));
        });
        self.sorted = Some(metric);
    }

    pub fn live_count(&self) -> u64 {
        self.nodes.len() as u64 - self.dead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn build() -> Tree {
        let mut tree = Tree::new(Path::new("/r"));
        let names = ["a", "b.txt"];
        let first = tree.nodes.len() as u32;
        for (index, name) in names.iter().enumerate() {
            let name_off = push_name(&mut tree.names, name.as_bytes());
            let dir = index == 0;
            tree.nodes.push(Node {
                size: if dir { 0 } else { 10 },
                alloc: if dir { 0 } else { 4096 },
                parent: 0,
                name_off,
                files: if dir { 0 } else { 1 },
                dirs: if dir { 1 } else { 0 },
                flags: NodeFlags(if dir { NodeFlags::DIR } else { 0 }),
                ..Node::default()
            });
            tree.order.push(first + index as u32);
        }
        tree.nodes[0].first_child = first;
        tree.nodes[0].child_count = 2;
        tree.add_up(
            0,
            Totals {
                size: 10,
                alloc: 4096,
                files: 1,
                dirs: 1,
                mtime: 0,
            },
        );
        let inner = tree.nodes.len() as u32;
        let name_off = push_name(&mut tree.names, b"big.bin");
        tree.nodes.push(Node {
            size: 100,
            alloc: 8192,
            parent: 1,
            name_off,
            files: 1,
            ..Node::default()
        });
        tree.order.push(inner);
        tree.nodes[1].first_child = inner;
        tree.nodes[1].child_count = 1;
        tree.add_up(
            1,
            Totals {
                size: 100,
                alloc: 8192,
                files: 1,
                dirs: 0,
                mtime: 5,
            },
        );
        tree
    }

    #[test]
    fn names_round_trip_short_and_long() {
        let mut names = Vec::new();
        let long = vec![b'x'; 400];
        let a = push_name(&mut names, b"hello");
        let b = push_name(&mut names, &long);
        assert_eq!(read_name(&names, a), b"hello");
        assert_eq!(read_name(&names, b), long.as_slice());
    }

    #[test]
    fn totals_paths_and_lookup() {
        let tree = build();
        assert_eq!(tree.node(0).size, 110);
        assert_eq!(tree.node(0).alloc, 12288);
        assert_eq!(tree.node(0).files, 2);
        assert_eq!(tree.node(0).folders(), 1);
        assert_eq!(tree.node(0).mtime, 5);
        assert_eq!(tree.path(3), PathBuf::from("/r/a/big.bin"));
        assert_eq!(tree.find_path(Path::new("/r/a/big.bin")), Some(3));
        assert!(tree.is_ancestor(1, 3));
        assert!(!tree.is_ancestor(2, 3));
    }

    #[test]
    fn sorting_and_removal_keep_totals() {
        let mut tree = build();
        tree.sort_all(Metric::Apparent);
        assert_eq!(&tree.order[1..3], &[1, 2]);
        tree.remove(1);
        assert_eq!(tree.node(0).size, 10);
        assert_eq!(tree.node(0).files, 1);
        assert_eq!(tree.node(0).folders(), 0);
        assert!(tree.node(3).is_dead());
        assert_eq!(tree.live_children(0).collect::<Vec<_>>(), vec![2]);
        assert_eq!(tree.order[1], 2);
    }
}
