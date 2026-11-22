use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rayon::prelude::*;

use crate::tree::{Metric, NodeFlags, NodeId, Tree};

pub fn largest_files(tree: &Tree, within: NodeId, metric: Metric, limit: usize) -> Vec<NodeId> {
    if limit == 0 {
        return Vec::new();
    }
    let push = |mut heap: BinaryHeap<Reverse<(u64, NodeId)>>, id: u32| {
        let node = &tree.nodes[id as usize];
        if node.is_dir() || node.flags.has(NodeFlags::DEAD) {
            return heap;
        }
        let weight = node.weight(metric);
        if weight == 0 || (heap.len() >= limit && heap.peek().is_some_and(|min| min.0.0 >= weight)) {
            return heap;
        }
        if within != tree.root && !tree.is_ancestor(within, id) {
            return heap;
        }
        heap.push(Reverse((weight, id)));
        if heap.len() > limit {
            heap.pop();
        }
        heap
    };
    let heap = (1..tree.nodes.len() as u32)
        .into_par_iter()
        .with_min_len(8192)
        .fold(BinaryHeap::new, push)
        .reduce(BinaryHeap::new, |mut a, b| {
            for item in b {
                a.push(item);
                if a.len() > limit {
                    a.pop();
                }
            }
            a
        });
    let mut items: Vec<(u64, NodeId)> = heap.into_iter().map(|Reverse(item)| item).collect();
    items.sort_unstable_by_key(|&(weight, id)| (Reverse(weight), id));
    items.into_iter().map(|(_, id)| id).collect()
}
