use std::cmp::Reverse;

use rustc_hash::FxHashMap;

use crate::tree::Metric;

pub const NO_EXT: u16 = 0;
pub const MAX_EXT_LEN: usize = 16;
pub const NO_EXT_COLOR: [u8; 3] = [0x9A, 0x9A, 0xA4];

pub const PALETTE: [[u8; 3]; 16] = [
    [0x4E, 0x7C, 0xF6],
    [0xF2, 0x5C, 0x54],
    [0x5C, 0xC9, 0x6B],
    [0xF2, 0xC1, 0x4E],
    [0x3F, 0xC7, 0xC9],
    [0xC0, 0x62, 0xE0],
    [0xF2, 0x88, 0x4B],
    [0x8C, 0x9C, 0xFF],
    [0xFF, 0x8F, 0xAB],
    [0x9B, 0xDE, 0x6A],
    [0x6F, 0xD6, 0xF2],
    [0xE3, 0x8B, 0xF5],
    [0xFF, 0xD9, 0x8A],
    [0x5F, 0xD3, 0xBC],
    [0xC8, 0xA2, 0x7A],
    [0xA8, 0xB4, 0xCC],
];

#[derive(Clone, Debug, Default)]
pub struct ExtStat {
    pub name: String,
    pub files: u64,
    pub size: u64,
    pub alloc: u64,
    pub color: [u8; 3],
}

impl ExtStat {
    pub fn weight(&self, metric: Metric) -> u64 {
        match metric {
            Metric::Allocated => self.alloc,
            Metric::Apparent => self.size,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExtTable {
    map: FxHashMap<Box<[u8]>, u16>,
    pub stats: Vec<ExtStat>,
}

impl Default for ExtTable {
    fn default() -> Self {
        ExtTable::new()
    }
}

pub fn ext_of(name: &[u8]) -> Option<&[u8]> {
    let mut end = name.len();
    loop {
        let dot = memchr::memrchr(b'.', &name[..end])?;
        let ext = &name[dot + 1..end];
        if dot == 0 || ext.is_empty() || ext.len() > MAX_EXT_LEN {
            return None;
        }
        if !ext.iter().all(u8::is_ascii_digit) {
            return Some(ext);
        }
        end = dot;
    }
}

fn hashed_color(name: &str) -> [u8; 3] {
    let mut hash: u32 = 2166136261;
    for byte in name.bytes() {
        hash = (hash ^ byte as u32).wrapping_mul(16777619);
    }
    let hue = (hash % 360) as f32;
    hsv(hue, 0.38, 0.78)
}

pub fn hsv(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let c = value * saturation;
    let h = hue / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    [
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    ]
}

impl ExtTable {
    pub fn new() -> ExtTable {
        ExtTable {
            map: FxHashMap::default(),
            stats: vec![ExtStat {
                name: String::new(),
                color: NO_EXT_COLOR,
                ..ExtStat::default()
            }],
        }
    }

    pub fn intern_name(&mut self, file_name: &[u8]) -> u16 {
        match ext_of(file_name) {
            Some(ext) => self.intern(ext),
            None => NO_EXT,
        }
    }

    pub fn intern(&mut self, ext: &[u8]) -> u16 {
        let mut buffer = [0u8; MAX_EXT_LEN];
        let len = ext.len().min(MAX_EXT_LEN);
        for (slot, byte) in buffer.iter_mut().zip(ext) {
            *slot = byte.to_ascii_lowercase();
        }
        let key = &buffer[..len];
        if let Some(&id) = self.map.get(key) {
            return id;
        }
        if self.stats.len() >= u16::MAX as usize {
            return NO_EXT;
        }
        let id = self.stats.len() as u16;
        let name = String::from_utf8_lossy(key).into_owned();
        self.stats.push(ExtStat {
            color: hashed_color(&name),
            name,
            ..ExtStat::default()
        });
        self.map.insert(key.into(), id);
        id
    }

    pub fn add(&mut self, id: u16, size: u64, alloc: u64) {
        let stat = &mut self.stats[id as usize];
        stat.files += 1;
        stat.size += size;
        stat.alloc += alloc;
    }

    pub fn sub(&mut self, id: u16, size: u64, alloc: u64) {
        let Some(stat) = self.stats.get_mut(id as usize) else {
            return;
        };
        stat.files = stat.files.saturating_sub(1);
        stat.size = stat.size.saturating_sub(size);
        stat.alloc = stat.alloc.saturating_sub(alloc);
    }

    pub fn find(&self, ext: &str) -> Option<u16> {
        let lower = ext.trim_start_matches('.').to_ascii_lowercase();
        if lower.is_empty() {
            return Some(NO_EXT);
        }
        self.map.get(lower.as_bytes()).copied()
    }

    pub fn ranked(&self, metric: Metric) -> Vec<u16> {
        let mut ids: Vec<u16> = (0..self.stats.len() as u16)
            .filter(|&id| self.stats[id as usize].files > 0)
            .collect();
        ids.sort_unstable_by_key(|&id| Reverse(self.stats[id as usize].weight(metric)));
        ids
    }

    pub fn assign_colors(&mut self, metric: Metric) {
        let ranked = self.ranked(metric);
        let mut next = 0;
        for id in ranked {
            if id == NO_EXT {
                continue;
            }
            let stat = &mut self.stats[id as usize];
            stat.color = match PALETTE.get(next) {
                Some(color) => *color,
                None => hashed_color(&stat.name),
            };
            next += 1;
        }
    }

    pub fn colors(&self) -> Vec<[u8; 3]> {
        self.stats.iter().map(|s| s.color).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_follow_the_last_dot() {
        assert_eq!(ext_of(b"a.tar.gz"), Some(&b"gz"[..]));
        assert_eq!(ext_of(b".bashrc"), None);
        assert_eq!(ext_of(b"trailing."), None);
        assert_eq!(ext_of(b"noext"), None);
        assert_eq!(ext_of(b"x.abcdefghijklmnopq"), None);
        assert_eq!(ext_of(b"libfoo.so.1.2"), Some(&b"so"[..]));
        assert_eq!(ext_of(b"backup.001"), None);
        assert_eq!(ext_of(b"archive.7z"), Some(&b"7z"[..]));
    }

    #[test]
    fn interning_is_case_insensitive_and_ranked_colors_are_distinct() {
        let mut table = ExtTable::new();
        let a = table.intern(b"JPG");
        let b = table.intern(b"jpg");
        let c = table.intern(b"mp4");
        assert_eq!(a, b);
        assert_ne!(a, c);
        table.add(a, 10, 10);
        table.add(c, 100, 100);
        table.assign_colors(Metric::Apparent);
        assert_eq!(table.ranked(Metric::Apparent), vec![c, a]);
        assert_eq!(table.stats[c as usize].color, PALETTE[0]);
        assert_eq!(table.stats[a as usize].color, PALETTE[1]);
        assert_eq!(table.find(".JPG"), Some(a));
    }
}
