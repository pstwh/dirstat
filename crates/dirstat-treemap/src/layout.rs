use dirstat_core::{Metric, NodeId, Tree};

use crate::squarify::{Rect, squarify};

pub const BAND: u32 = 32;
pub const AGGREGATE: u8 = 1;
pub const DIRECTORY: u8 = 2;
pub const AGGREGATE_COLOR: [u8; 3] = [0x86, 0x88, 0x92];
pub const EMPTY_COLOR: [u8; 3] = [0x5C, 0x5E, 0x66];

#[derive(Clone, Debug)]
pub struct LayoutOptions {
    pub width: u32,
    pub height: u32,
    pub metric: Metric,
    pub height_factor: f32,
    pub scale: f32,
    pub min_side: f32,
    pub min_area: f32,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        LayoutOptions {
            width: 800,
            height: 600,
            metric: Metric::Allocated,
            height_factor: 0.38,
            scale: 0.91,
            min_side: 2.0,
            min_area: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tile {
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
    pub node: NodeId,
    pub color: [u8; 3],
    pub flags: u8,
    pub ext: u16,
    pub surface: [f32; 4],
}

impl Tile {
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x0 as u32 && x < self.x1 as u32 && y >= self.y0 as u32 && y < self.y1 as u32
    }

    pub fn rect(&self) -> [u16; 4] {
        [self.x0, self.y0, self.x1, self.y1]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub node: NodeId,
    pub rect: [u16; 4],
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub root: NodeId,
    pub tiles: Vec<Tile>,
    pub frames: Vec<Frame>,
    pub band_starts: Vec<u32>,
    pub band_tiles: Vec<u32>,
}

struct Item {
    node: NodeId,
    rect: Rect,
    surface: [f32; 4],
    height: f32,
}

fn add_ridge(rect: &Rect, surface: &mut [f32; 4], height: f32) {
    let width = (rect[2] - rect[0]) as f32;
    let tall = (rect[3] - rect[1]) as f32;
    if width > 0.0 {
        let factor = 4.0 * height / width;
        surface[2] += factor * (rect[2] + rect[0]) as f32;
        surface[0] -= factor;
    }
    if tall > 0.0 {
        let factor = 4.0 * height / tall;
        surface[3] += factor * (rect[3] + rect[1]) as f32;
        surface[1] -= factor;
    }
}

fn snap(rect: &Rect) -> [u16; 4] {
    [
        rect[0].round().clamp(0.0, 65535.0) as u16,
        rect[1].round().clamp(0.0, 65535.0) as u16,
        rect[2].round().clamp(0.0, 65535.0) as u16,
        rect[3].round().clamp(0.0, 65535.0) as u16,
    ]
}

pub fn layout(tree: &Tree, root: NodeId, options: &LayoutOptions) -> Layout {
    let mut result = Layout {
        width: options.width,
        height: options.height,
        root,
        ..Layout::default()
    };
    let colors: Vec<[u8; 3]> = tree.exts.colors();
    let metric = options.metric;
    let mut scratch: Vec<u32> = Vec::new();
    let mut weights: Vec<f64> = Vec::new();
    let mut ids: Vec<NodeId> = Vec::new();
    let mut rects: Vec<Rect> = Vec::new();
    let mut stack = vec![Item {
        node: root,
        rect: [0.0, 0.0, options.width as f64, options.height as f64],
        surface: [0.0; 4],
        height: options.height_factor,
    }];
    let emit = |tiles: &mut Vec<Tile>, node: NodeId, rect: &Rect, surface: [f32; 4], color, flags, ext| {
        let [x0, y0, x1, y1] = snap(rect);
        if x1 > x0 && y1 > y0 {
            tiles.push(Tile {
                x0,
                y0,
                x1,
                y1,
                node,
                color,
                flags,
                ext,
                surface,
            });
        }
    };
    while let Some(item) = stack.pop() {
        let node = tree.node(item.node);
        let mut surface = item.surface;
        add_ridge(&item.rect, &mut surface, item.height);
        let width = item.rect[2] - item.rect[0];
        let height = item.rect[3] - item.rect[1];
        if !node.is_dir() {
            let color = colors.get(node.ext as usize).copied().unwrap_or(AGGREGATE_COLOR);
            emit(&mut result.tiles, item.node, &item.rect, surface, color, 0, node.ext);
            continue;
        }
        let small = (width.min(height) as f32) < options.min_side;
        if small || node.child_count == 0 {
            let color = if node.child_count == 0 {
                EMPTY_COLOR
            } else {
                AGGREGATE_COLOR
            };
            emit(
                &mut result.tiles,
                item.node,
                &item.rect,
                surface,
                color,
                AGGREGATE | DIRECTORY,
                0,
            );
            continue;
        }
        result.frames.push(Frame {
            node: item.node,
            rect: snap(&item.rect),
        });
        weights.clear();
        ids.clear();
        for &child in tree.sorted_children(item.node, metric, &mut scratch) {
            let child_node = tree.node(child);
            if child_node.is_dead() {
                continue;
            }
            let weight = child_node.weight(metric);
            if weight == 0 {
                continue;
            }
            weights.push(weight as f64);
            ids.push(child);
        }
        let inner = item.rect;
        if weights.is_empty() {
            emit(
                &mut result.tiles,
                item.node,
                &item.rect,
                surface,
                EMPTY_COLOR,
                AGGREGATE | DIRECTORY,
                0,
            );
            continue;
        }
        let rest = squarify(&weights, inner, options.min_area as f64, &mut rects);
        let child_height = item.height * options.scale;
        for (rect, &child) in rects.iter().zip(ids.iter()) {
            stack.push(Item {
                node: child,
                rect: *rect,
                surface,
                height: child_height,
            });
        }
        if let Some(rest) = rest {
            let mut rest_surface = surface;
            add_ridge(&rest, &mut rest_surface, child_height);
            emit(
                &mut result.tiles,
                item.node,
                &rest,
                rest_surface,
                AGGREGATE_COLOR,
                AGGREGATE | DIRECTORY,
                0,
            );
        }
    }
    result.frames.sort_unstable_by_key(|f| f.node);
    result.build_bands();
    result
}

impl Layout {
    pub fn band_count(&self) -> usize {
        self.height.div_ceil(BAND) as usize
    }

    fn build_bands(&mut self) {
        let bands = self.band_count();
        let mut counts = vec![0u32; bands + 1];
        for tile in &self.tiles {
            let first = tile.y0 as u32 / BAND;
            let last = (tile.y1 as u32 - 1) / BAND;
            for band in first..=last.min(bands as u32 - 1) {
                counts[band as usize + 1] += 1;
            }
        }
        for index in 1..counts.len() {
            counts[index] += counts[index - 1];
        }
        let mut fill = counts.clone();
        let mut band_tiles = vec![0u32; *counts.last().unwrap_or(&0) as usize];
        for (index, tile) in self.tiles.iter().enumerate() {
            let first = tile.y0 as u32 / BAND;
            let last = (tile.y1 as u32 - 1) / BAND;
            for band in first..=last.min(bands as u32 - 1) {
                let slot = &mut fill[band as usize];
                band_tiles[*slot as usize] = index as u32;
                *slot += 1;
            }
        }
        self.band_starts = counts;
        self.band_tiles = band_tiles;
    }

    pub fn band(&self, band: usize) -> &[u32] {
        let start = self.band_starts[band] as usize;
        let end = self.band_starts[band + 1] as usize;
        &self.band_tiles[start..end]
    }

    pub fn hit(&self, x: u32, y: u32) -> Option<&Tile> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let band = (y / BAND) as usize;
        self.band(band)
            .iter()
            .map(|&index| &self.tiles[index as usize])
            .find(|tile| tile.contains(x, y))
    }

    pub fn frame_of(&self, node: NodeId) -> Option<[u16; 4]> {
        self.frames
            .binary_search_by_key(&node, |f| f.node)
            .ok()
            .map(|index| self.frames[index].rect)
    }

    pub fn rect_of(&self, node: NodeId) -> Option<[u16; 4]> {
        self.frame_of(node)
            .or_else(|| self.tiles.iter().find(|t| t.node == node).map(|t| t.rect()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use dirstat_core::tree::{Node, NodeFlags, Totals, push_name};

    use super::*;

    fn synthetic(files: usize) -> Tree {
        let mut tree = Tree::new(Path::new("/s"));
        let dirs = 50;
        let first = tree.nodes.len() as u32;
        for d in 0..dirs {
            let name_off = push_name(&mut tree.names, format!("d{d}").as_bytes());
            tree.nodes.push(Node {
                parent: 0,
                name_off,
                dirs: 1,
                flags: NodeFlags(NodeFlags::DIR),
                ..Node::default()
            });
            tree.order.push(first + d);
        }
        tree.nodes[0].first_child = first;
        tree.nodes[0].child_count = dirs;
        tree.add_up(
            0,
            Totals {
                dirs,
                ..Totals::default()
            },
        );
        for d in 0..dirs {
            let parent = first + d;
            let start = tree.nodes.len() as u32;
            let per = files / dirs as usize;
            let mut totals = Totals::default();
            for f in 0..per {
                let size = ((f * 7919 + d as usize * 31) % 100_000 + 1) as u64;
                let name_off = push_name(&mut tree.names, format!("f{f}.bin").as_bytes());
                tree.nodes.push(Node {
                    size,
                    alloc: size,
                    parent,
                    name_off,
                    files: 1,
                    ..Node::default()
                });
                tree.order.push(start + f as u32);
                totals.size += size;
                totals.alloc += size;
                totals.files += 1;
            }
            tree.nodes[parent as usize].first_child = start;
            tree.nodes[parent as usize].child_count = per as u32;
            tree.add_up(parent, totals);
        }
        tree.sort_all(Metric::Allocated);
        tree
    }

    #[test]
    fn tiles_stay_inside_and_do_not_overlap() {
        let tree = synthetic(2000);
        let options = LayoutOptions {
            width: 300,
            height: 200,
            ..LayoutOptions::default()
        };
        let layout = layout(&tree, 0, &options);
        let mut coverage = vec![0u8; 300 * 200];
        for tile in &layout.tiles {
            assert!(tile.x1 as u32 <= 300 && tile.y1 as u32 <= 200);
            for y in tile.y0..tile.y1 {
                for x in tile.x0..tile.x1 {
                    coverage[y as usize * 300 + x as usize] += 1;
                }
            }
        }
        assert!(coverage.iter().all(|&c| c == 1));
    }

    #[test]
    fn culling_bounds_tile_count_and_hit_matches_brute_force() {
        let tree = synthetic(500_000);
        let options = LayoutOptions {
            width: 640,
            height: 480,
            ..LayoutOptions::default()
        };
        let layout = layout(&tree, 0, &options);
        assert!(layout.tiles.len() < 640 * 480 / 2, "{}", layout.tiles.len());
        let mut seed = 12345u32;
        for _ in 0..500 {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let x = seed % 640;
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let y = seed % 480;
            let brute = layout.tiles.iter().find(|t| t.contains(x, y)).map(|t| t.node);
            assert_eq!(layout.hit(x, y).map(|t| t.node), brute);
            assert!(brute.is_some());
        }
        let first_dir = tree.order[1];
        assert!(layout.rect_of(first_dir).is_some());
    }
}
