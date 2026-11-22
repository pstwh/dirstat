use std::cell::RefCell;
use std::cmp::Reverse;

use memchr::memmem::Finder;
use rayon::prelude::*;

use crate::format::{now_secs, parse_age, parse_size};
use crate::tree::{Metric, NodeId, Tree};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub terms: Vec<String>,
    pub exts: Vec<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub older_than: Option<u32>,
    pub newer_than: Option<u32>,
    pub dirs: Option<bool>,
}

impl Query {
    pub fn parse(text: &str) -> Query {
        let mut query = Query::default();
        for token in text.split_whitespace() {
            let lower = token.to_lowercase();
            if let Some(list) = lower.strip_prefix("ext:") {
                query
                    .exts
                    .extend(list.split(',').map(|e| e.trim_start_matches('.').to_string()));
            } else if let Some(rule) = lower.strip_prefix("size:") {
                if let Some(value) = rule.strip_prefix(">=").or_else(|| rule.strip_prefix('>')) {
                    query.min_size = parse_size(value);
                } else if let Some(value) = rule.strip_prefix("<=").or_else(|| rule.strip_prefix('<')) {
                    query.max_size = parse_size(value);
                } else {
                    query.min_size = parse_size(rule);
                }
            } else if let Some(age) = lower.strip_prefix("older:") {
                query.older_than = parse_age(age);
            } else if let Some(age) = lower.strip_prefix("newer:") {
                query.newer_than = parse_age(age);
            } else if let Some(kind) = lower.strip_prefix("kind:").or_else(|| lower.strip_prefix("type:")) {
                query.dirs = match kind {
                    "dir" | "dirs" | "folder" | "folders" | "d" => Some(true),
                    "file" | "files" | "f" => Some(false),
                    _ => None,
                };
            } else if let Some(ext) = lower.strip_prefix("*.") {
                query.exts.push(ext.to_string());
            } else {
                query.terms.push(lower);
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        *self == Query::default()
    }
}

thread_local! {
    static LOWER: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(256));
}

pub fn search(tree: &Tree, query: &Query, within: NodeId, metric: Metric, limit: usize) -> Vec<NodeId> {
    if query.is_empty() {
        return Vec::new();
    }
    let exts: Vec<u16> = query.exts.iter().filter_map(|e| tree.exts.find(e)).collect();
    if !query.exts.is_empty() && exts.is_empty() {
        return Vec::new();
    }
    let finders: Vec<Finder> = query.terms.iter().map(|t| Finder::new(t.as_bytes())).collect();
    let now = now_secs();
    let older = query.older_than.map(|age| now.saturating_sub(age));
    let newer = query.newer_than.map(|age| now.saturating_sub(age));
    let mut hits: Vec<(u64, NodeId)> = (1..tree.nodes.len() as u32)
        .into_par_iter()
        .with_min_len(4096)
        .filter_map(|id| {
            let node = &tree.nodes[id as usize];
            if node.is_dead() {
                return None;
            }
            if let Some(dirs) = query.dirs
                && node.is_dir() != dirs
            {
                return None;
            }
            if !exts.is_empty() && (node.is_dir() || !exts.contains(&node.ext)) {
                return None;
            }
            let weight = node.weight(metric);
            if query.min_size.is_some_and(|min| weight < min) || query.max_size.is_some_and(|max| weight > max) {
                return None;
            }
            if older.is_some_and(|limit| node.mtime == 0 || node.mtime > limit) {
                return None;
            }
            if newer.is_some_and(|limit| node.mtime < limit) {
                return None;
            }
            if !finders.is_empty() {
                let name = tree.name_bytes(id);
                let found = LOWER.with_borrow_mut(|lower| {
                    lower.clear();
                    lower.extend(name.iter().map(|b| b.to_ascii_lowercase()));
                    finders.iter().all(|f| f.find(lower).is_some())
                });
                if !found {
                    return None;
                }
            }
            if within != tree.root && !tree.is_ancestor(within, id) {
                return None;
            }
            Some((weight, id))
        })
        .collect();
    if hits.len() > limit {
        hits.select_nth_unstable_by_key(limit, |&(weight, _)| Reverse(weight));
        hits.truncate(limit);
    }
    hits.par_sort_unstable_by_key(|&(weight, id)| (Reverse(weight), id));
    hits.into_iter().map(|(_, id)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_parse_filters_and_terms() {
        let query = Query::parse("Report ext:pdf,DOCX size:>10mb older:1y kind:file *.txt");
        assert_eq!(query.terms, vec!["report".to_string()]);
        assert_eq!(query.exts, vec!["pdf", "docx", "txt"]);
        assert_eq!(query.min_size, Some(10 * 1024 * 1024));
        assert_eq!(query.older_than, Some(365 * 86400));
        assert_eq!(query.dirs, Some(false));
        assert!(Query::parse("  ").is_empty());
    }
}
