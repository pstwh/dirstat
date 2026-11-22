use std::time::Instant;

use dirstat_core::scan::{ScanOptions, scan_blocking};
use dirstat_treemap::{LayoutOptions, RasterOptions, layout, rasterize};

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| "/usr".into());
    let (tree, _) = scan_blocking(ScanOptions::new(&root)).expect("scan");
    println!("entries {}", tree.len());
    let mut pixels = Vec::new();
    for (width, height) in [(1920u32, 1080u32), (3840, 2160)] {
        let options = LayoutOptions {
            width,
            height,
            ..LayoutOptions::default()
        };
        let started = Instant::now();
        let result = layout(&tree, 0, &options);
        let layout_ms = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        rasterize(&result, &RasterOptions::default(), &mut pixels);
        let raster_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{width}x{height}: {} tiles, layout {layout_ms:.1} ms, raster {raster_ms:.1} ms",
            result.tiles.len()
        );
    }
}
