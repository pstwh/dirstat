use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use dirstat_core::format::{human_count, human_date, human_size};
use dirstat_core::{NONE, NodeId, SharedTree};
use dirstat_treemap::{Layout, LayoutOptions, RasterOptions, layout, rasterize};
use egui::{
    Align2, Color32, ColorImage, Context, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui,
    pos2,
};

use crate::actions;
use crate::app::{Action, App};
use crate::icons;
use crate::widgets;

struct Job {
    tree: SharedTree,
    root: NodeId,
    layout: LayoutOptions,
    raster: RasterOptions,
    key: u64,
    epoch: u64,
}

struct Rendered {
    layout: Arc<Layout>,
    image: ColorImage,
    key: u64,
    epoch: u64,
}

pub struct TreemapView {
    sender: Sender<Job>,
    receiver: Receiver<Rendered>,
    texture: Option<TextureHandle>,
    pub layout: Option<Arc<Layout>>,
    requested: u64,
    last_request: Instant,
    last_size: [u32; 2],
    resized_at: Instant,
    menu_node: Option<NodeId>,
    pub shown: u64,
    epoch: u64,
}

fn worker(ctx: Context, jobs: Receiver<Job>, results: Sender<Rendered>) {
    let mut pixels: Vec<[u8; 4]> = Vec::new();
    while let Ok(mut job) = jobs.recv() {
        while let Ok(newer) = jobs.try_recv() {
            job = newer;
        }
        let computed = {
            let tree = job.tree.read();
            if tree.get(job.root).is_none() {
                continue;
            }
            layout(&tree, job.root, &job.layout)
        };
        rasterize(&computed, &job.raster, &mut pixels);
        let size = [computed.width as usize, computed.height as usize];
        let colors: Vec<Color32> = pixels.iter().map(|p| Color32::from_rgb(p[0], p[1], p[2])).collect();
        let rendered = Rendered {
            layout: Arc::new(computed),
            image: ColorImage::new(size, colors),
            key: job.key,
            epoch: job.epoch,
        };
        if results.send(rendered).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

impl TreemapView {
    pub fn new(ctx: &Context) -> TreemapView {
        let (sender, jobs) = unbounded::<Job>();
        let (results, receiver) = unbounded::<Rendered>();
        let worker_ctx = ctx.clone();
        std::thread::Builder::new()
            .name("dirstat-treemap".into())
            .spawn(move || worker(worker_ctx, jobs, results))
            .ok();
        TreemapView {
            sender,
            receiver,
            texture: None,
            layout: None,
            requested: 0,
            last_request: Instant::now(),
            last_size: [0, 0],
            resized_at: Instant::now(),
            menu_node: None,
            shown: 0,
            epoch: 0,
        }
    }

    pub fn reset(&mut self) {
        self.layout = None;
        self.texture = None;
        self.requested = 0;
        self.epoch += 1;
    }

    fn poll(&mut self, ctx: &Context) {
        let mut latest = None;
        while let Ok(rendered) = self.receiver.try_recv() {
            if rendered.epoch == self.epoch {
                latest = Some(rendered);
            }
        }
        let Some(rendered) = latest else { return };
        match &mut self.texture {
            Some(texture) => texture.set(rendered.image, TextureOptions::NEAREST),
            None => {
                self.texture = Some(ctx.load_texture("dirstat-treemap", rendered.image, TextureOptions::NEAREST));
            }
        }
        self.layout = Some(rendered.layout);
        self.shown = rendered.key;
    }
}

fn to_screen(rect: Rect, layout: &Layout, r: [u16; 4]) -> Rect {
    let sx = rect.width() / layout.width.max(1) as f32;
    let sy = rect.height() / layout.height.max(1) as f32;
    Rect::from_min_max(
        pos2(rect.min.x + r[0] as f32 * sx, rect.min.y + r[1] as f32 * sy),
        pos2(rect.min.x + r[2] as f32 * sx, rect.min.y + r[3] as f32 * sy),
    )
}

fn to_pixel(rect: Rect, layout: &Layout, pos: Pos2) -> (u32, u32) {
    let x = (pos.x - rect.min.x) / rect.width() * layout.width as f32;
    let y = (pos.y - rect.min.y) / rect.height() * layout.height as f32;
    (x.max(0.0) as u32, y.max(0.0) as u32)
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let theme = app.theme.clone();
    let Some(tree) = app.shared() else { return };
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click());
    app.treemap.poll(&ctx);
    let ppp = ctx.pixels_per_point();
    let max_side = ctx.input(|i| i.max_texture_side).min(8192) as f32;
    let width = (rect.width() * ppp).round().clamp(1.0, max_side) as u32;
    let height = (rect.height() * ppp).round().clamp(1.0, max_side) as u32;
    let size = [width, height];
    let now = Instant::now();
    if size != app.treemap.last_size {
        app.treemap.last_size = size;
        app.treemap.resized_at = now;
    }
    let scanning = app.is_scanning();
    let generation = tree.read().generation;
    let tick = if scanning {
        (now.duration_since(app.treemap.last_request).as_millis() / 700) as u64
    } else {
        0
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (
        app.zoom,
        size,
        app.settings.metric,
        app.settings.cushion,
        app.settings.brightness.to_bits(),
        app.settings.cushion_height.to_bits(),
        app.highlight_ext,
        theme.dark,
        generation,
    )
        .hash(&mut hasher);
    let key = hasher.finish().max(1);
    let settled =
        now.duration_since(app.treemap.resized_at) > Duration::from_millis(70) || app.treemap.texture.is_none();
    if (key != app.treemap.requested || tick > 0) && settled {
        let job = Job {
            tree: tree.clone(),
            root: app.zoom,
            layout: LayoutOptions {
                width,
                height,
                metric: app.settings.metric,
                height_factor: app.settings.cushion_height,
                ..LayoutOptions::default()
            },
            raster: RasterOptions {
                cushion: app.settings.cushion,
                brightness: app.settings.brightness,
                highlight_ext: app.highlight_ext,
                background: theme.base_rgb(),
                ..RasterOptions::default()
            },
            key,
            epoch: app.treemap.epoch,
        };
        if app.treemap.sender.send(job).is_ok() {
            app.treemap.requested = key;
            app.treemap.last_request = now;
        }
    }
    if !settled || scanning {
        ctx.request_repaint_after(Duration::from_millis(80));
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, theme.base);
    if let Some(texture) = &app.treemap.texture {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            format!("{}  Drawing", icons::TREEMAP),
            theme.ui(13.0),
            theme.faint,
        );
        return;
    }
    let Some(current) = app.treemap.layout.clone() else {
        return;
    };
    let tree_guard = tree.read();
    let hovered_tile = response
        .hover_pos()
        .and_then(|pos| {
            let (x, y) = to_pixel(rect, &current, pos);
            current.hit(x, y).copied()
        })
        .filter(|tile| tree_guard.get(tile.node).is_some());
    let list_hover = app.hovered.filter(|_| !response.hovered());
    if response.hovered() {
        app.hovered = hovered_tile.map(|t| t.node);
    }
    let outline = |painter: &egui::Painter, r: Rect, width: f32, color: Color32| {
        painter.rect_stroke(r.shrink(width / 2.0), 0, Stroke::new(width, color), StrokeKind::Inside);
    };
    if let Some(selected) = app.selected
        && tree_guard.get(selected).is_some()
        && tree_guard.is_ancestor(current.root, selected)
    {
        let mut node = selected;
        let mut found = None;
        while node != NONE {
            if let Some(r) = current.rect_of(node) {
                found = Some(r);
                break;
            }
            if node == current.root {
                break;
            }
            node = tree_guard.node(node).parent;
        }
        if let Some(r) = found {
            let screen = to_screen(rect, &current, r).intersect(rect);
            outline(&painter, screen.expand(1.0), 4.0, Color32::from_black_alpha(200));
            outline(&painter, screen, 2.0, Color32::WHITE);
        }
    }
    if let Some(node) = list_hover
        && tree_guard.get(node).is_some()
        && tree_guard.is_ancestor(current.root, node)
    {
        let mut target = node;
        while target != NONE && current.rect_of(target).is_none() && target != current.root {
            target = tree_guard.node(target).parent;
        }
        if let Some(r) = current.rect_of(target) {
            let screen = to_screen(rect, &current, r).intersect(rect);
            painter.rect_filled(screen, 0, theme.accent.gamma_multiply(0.18));
            outline(&painter, screen, 2.0, theme.accent);
        }
    }
    if let Some(tile) = hovered_tile {
        let parent = tree_guard.node(tile.node).parent;
        if tile.node != current.root
            && parent != NONE
            && let Some(frame) = current.frame_of(parent)
            && parent != current.root
        {
            outline(
                &painter,
                to_screen(rect, &current, frame),
                1.0,
                Color32::from_white_alpha(140),
            );
        }
        outline(
            &painter,
            to_screen(rect, &current, tile.rect()),
            1.0,
            Color32::from_white_alpha(230),
        );
    }
    if response.hovered()
        && !egui::Popup::is_any_open(&ctx)
        && let Some(tile) = hovered_tile
    {
        let node = tree_guard.node(tile.node);
        let path = tree_guard.path(tile.node);
        let total = tree_guard
            .get(current.root)
            .map(|n| n.weight(app.settings.metric))
            .unwrap_or(0)
            .max(1);
        let weight = node.weight(app.settings.metric);
        let share = weight as f64 / total as f64 * 100.0;
        let mut lines = vec![format!("{}  {share:.1}%", human_size(weight))];
        if node.is_dir() {
            lines.push(format!(
                "{} files, {} folders",
                human_count(node.files as u64),
                human_count(node.folders() as u64)
            ));
        }
        let modified = human_date(node.mtime);
        if !modified.is_empty() {
            lines.push(format!("Modified {modified}"));
        }
        let path_text = widgets::truncate_middle(&path.display().to_string(), 90);
        let theme_tip = theme.clone();
        egui::Tooltip::always_open(
            ctx.clone(),
            ui.layer_id(),
            egui::Id::unique("treemap-tip"),
            egui::PopupAnchor::Pointer,
        )
        .gap(14.0)
        .show(|ui| {
            ui.set_max_width(520.0);
            ui.label(
                egui::RichText::new(path_text)
                    .font(theme_tip.medium(12.5))
                    .color(theme_tip.fg),
            );
            for line in lines {
                ui.label(widgets::small(&theme_tip, line));
            }
        });
    }
    let total_scroll = ui.input(|i| i.smooth_scroll_delta.y);
    drop(tree_guard);
    if response.hovered() && total_scroll.abs() > 0.5 {
        let consumed = ui.memory(|m| m.data.get_temp::<f64>(egui::Id::unique("treemap-wheel")).unwrap_or(0.0));
        let time = ui.input(|i| i.time);
        if time - consumed > 0.25 {
            ui.memory_mut(|m| m.data.insert_temp(egui::Id::unique("treemap-wheel"), time));
            if total_scroll > 0.0 {
                if let Some(tile) = hovered_tile {
                    let tree_guard = tree.read();
                    let chain = tree_guard.ancestors(tile.node);
                    if let Some(position) = chain.iter().position(|&n| n == app.zoom)
                        && let Some(&next) = chain.get(position + 1)
                        && tree_guard.node(next).is_dir()
                    {
                        app.actions.push(Action::Zoom(next));
                    }
                }
            } else {
                app.actions.push(Action::ZoomOut);
            }
        }
    }
    if let Some(tile) = hovered_tile {
        if response.double_clicked() {
            let tree_guard = tree.read();
            let entry = tree_guard.node(tile.node);
            let target = if entry.is_dir() { tile.node } else { entry.parent };
            drop(tree_guard);
            if target == app.zoom {
                app.actions.push(Action::ZoomOut);
            } else {
                app.actions.push(Action::Zoom(target));
            }
        } else if response.clicked() {
            app.actions.push(Action::Select(tile.node));
        }
        if response.secondary_clicked() {
            app.treemap.menu_node = Some(tile.node);
            app.actions.push(Action::Select(tile.node));
        }
        if response.middle_clicked() {
            app.actions.push(Action::Open(tile.node));
        }
    }
    let menu_node = app.treemap.menu_node;
    response.context_menu(|ui| {
        if let Some(node) = menu_node {
            let guard = tree.read();
            actions::context_menu(app, &guard, ui, node);
        }
    });
}
