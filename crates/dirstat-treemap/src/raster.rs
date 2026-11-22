use rayon::prelude::*;

use crate::layout::{BAND, Layout};

#[derive(Clone, Debug)]
pub struct RasterOptions {
    pub cushion: bool,
    pub brightness: f32,
    pub ambient: f32,
    pub light: [f32; 3],
    pub highlight_ext: Option<u16>,
    pub background: [u8; 3],
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions {
            cushion: true,
            brightness: 1.0,
            ambient: 0.15,
            light: [-1.0, -1.0, 10.0],
            highlight_ext: None,
            background: [0x16, 0x17, 0x1A],
        }
    }
}

pub fn rasterize(layout: &Layout, options: &RasterOptions, out: &mut Vec<[u8; 4]>) {
    let width = layout.width as usize;
    let height = layout.height as usize;
    let [r, g, b] = options.background;
    out.clear();
    out.resize(width * height, [r, g, b, 255]);
    if width == 0 || height == 0 {
        return;
    }
    let norm = (options.light[0].powi(2) + options.light[1].powi(2) + options.light[2].powi(2)).sqrt();
    let (lx, ly, lz) = (
        options.light[0] / norm,
        options.light[1] / norm,
        options.light[2] / norm,
    );
    let diffuse = 1.0 - options.ambient;
    let gain = options.brightness * 1.08;
    out.par_chunks_mut(width * BAND as usize)
        .enumerate()
        .for_each(|(band, rows)| {
            let band_y0 = band * BAND as usize;
            let band_y1 = band_y0 + rows.len() / width;
            for &index in layout.band(band) {
                let tile = &layout.tiles[index as usize];
                let y0 = (tile.y0 as usize).max(band_y0);
                let y1 = (tile.y1 as usize).min(band_y1);
                let (x0, x1) = (tile.x0 as usize, tile.x1 as usize);
                let dim = match options.highlight_ext {
                    Some(ext) if tile.ext != ext || tile.flags != 0 => 0.22,
                    _ => 1.0,
                };
                let base = [
                    tile.color[0] as f32 * gain * dim,
                    tile.color[1] as f32 * gain * dim,
                    tile.color[2] as f32 * gain * dim,
                ];
                let s = tile.surface;
                for y in y0..y1 {
                    let row = &mut rows[(y - band_y0) * width..(y - band_y0 + 1) * width];
                    if !options.cushion {
                        let edge_y = y + 1 == tile.y1 as usize;
                        for (x, pixel) in row.iter_mut().enumerate().take(x1).skip(x0) {
                            let shade = if edge_y || x + 1 == x1 { 0.62 } else { 0.92 };
                            *pixel = [
                                (base[0] * shade).min(255.0) as u8,
                                (base[1] * shade).min(255.0) as u8,
                                (base[2] * shade).min(255.0) as u8,
                                255,
                            ];
                        }
                        continue;
                    }
                    let ny = -(2.0 * s[1] * (y as f32 + 0.5) + s[3]);
                    let ny_term = ny * ly + lz;
                    let ny_sq = ny * ny + 1.0;
                    for (x, pixel) in row.iter_mut().enumerate().take(x1).skip(x0) {
                        let nx = -(2.0 * s[0] * (x as f32 + 0.5) + s[2]);
                        let cosa = (nx * lx + ny_term) / (nx * nx + ny_sq).sqrt();
                        let intensity = options.ambient + diffuse * cosa.max(0.0);
                        *pixel = [
                            (base[0] * intensity).min(255.0) as u8,
                            (base[1] * intensity).min(255.0) as u8,
                            (base[2] * intensity).min(255.0) as u8,
                            255,
                        ];
                    }
                }
            }
        });
}
