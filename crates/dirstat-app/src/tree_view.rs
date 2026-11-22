use std::cmp::Reverse;
use std::time::{Duration, Instant};

use dirstat_core::format::{human_count, human_date, human_size};
use dirstat_core::{Metric, NodeFlags, NodeId, Tree};
use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::actions;
use crate::app::{Action, App, Column, Row};
use crate::icons;
use crate::theme::Theme;
use crate::widgets;

pub const ROW_HEIGHT: f32 = 24.0;
const HEADER_HEIGHT: f32 = 26.0;

struct Columns {
    name: f32,
    bar: f32,
    percent: f32,
    size: f32,
    other: f32,
    items: f32,
    files: f32,
    modified: f32,
}

impl Columns {
    fn fit(width: f32) -> Columns {
        let mut columns = Columns {
            name: 0.0,
            bar: 64.0,
            percent: 52.0,
            size: 82.0,
            other: 82.0,
            items: 72.0,
            files: 72.0,
            modified: 118.0,
        };
        let min_name = 200.0;
        let fixed = |c: &Columns| c.bar + c.percent + c.size + c.other + c.items + c.files + c.modified;
        for drop in 0..5 {
            if width - fixed(&columns) >= min_name {
                break;
            }
            match drop {
                0 => columns.modified = 0.0,
                1 => columns.other = 0.0,
                2 => columns.items = 0.0,
                3 => columns.files = 0.0,
                _ => columns.bar = 0.0,
            }
        }
        columns.name = (width - fixed(&columns)).max(120.0);
        columns
    }
}

fn compare_names(tree: &Tree, a: NodeId, b: NodeId) -> std::cmp::Ordering {
    let left = tree.name_bytes(a).iter().map(|c| c.to_ascii_lowercase());
    let right = tree.name_bytes(b).iter().map(|c| c.to_ascii_lowercase());
    left.cmp(right)
}

pub fn sorted_children(tree: &Tree, id: NodeId, column: Column, descending: bool, metric: Metric) -> Vec<NodeId> {
    if column == Column::Size && descending && tree.sorted == Some(metric) {
        let range = tree.node(id).children();
        return tree.order[range.start as usize..range.end as usize]
            .iter()
            .copied()
            .filter(|&c| !tree.node(c).is_dead())
            .collect();
    }
    let mut children: Vec<NodeId> = tree.live_children(id).collect();
    let node = |c: NodeId| tree.node(c);
    match column {
        Column::Name => children.sort_by(|&a, &b| {
            node(b)
                .is_dir()
                .cmp(&node(a).is_dir())
                .then_with(|| compare_names(tree, a, b))
        }),
        Column::Size => children.sort_unstable_by_key(|&c| Reverse(node(c).weight(metric))),
        Column::Other => children.sort_unstable_by_key(|&c| Reverse(node(c).weight(metric.other()))),
        Column::Items => children.sort_unstable_by_key(|&c| Reverse(node(c).items())),
        Column::Files => children.sort_unstable_by_key(|&c| Reverse(node(c).files)),
        Column::Modified => children.sort_unstable_by_key(|&c| Reverse(node(c).mtime)),
    }
    let natural_descending = column != Column::Name;
    if descending != natural_descending {
        children.reverse();
    }
    children
}

pub fn rebuild_rows(app: &mut App, tree: &Tree) {
    let state = &mut app.folders;
    state.rows.clear();
    let mut stack = vec![Row { node: 0, depth: 0 }];
    while let Some(row) = stack.pop() {
        state.rows.push(row);
        let node = tree.node(row.node);
        if node.is_dir() && state.expanded.contains(&row.node) {
            let children = sorted_children(tree, row.node, state.sort, state.descending, app.settings.metric);
            for child in children.into_iter().rev() {
                stack.push(Row {
                    node: child,
                    depth: row.depth.saturating_add(1),
                });
            }
        }
    }
    state.dirty = false;
    state.built = Instant::now();
    state.built_generation = tree.generation;
}

fn header(ui: &mut Ui, app: &mut App, columns: &Columns, rect: Rect) {
    let theme = app.theme.clone();
    let painter = ui.painter();
    painter.rect_filled(rect, 0, theme.panel);
    painter.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, theme.line));
    let metric = app.settings.metric;
    let entries: [(Column, &str, f32, bool); 7] = [
        (Column::Name, "Name", columns.name + columns.bar, false),
        (Column::Size, "%", columns.percent, true),
        (Column::Size, metric.label(), columns.size, true),
        (Column::Other, metric.other().label(), columns.other, true),
        (Column::Items, "Items", columns.items, true),
        (Column::Files, "Files", columns.files, true),
        (Column::Modified, "Modified", columns.modified, true),
    ];
    let mut x = rect.min.x;
    for (index, (column, label, width, right)) in entries.into_iter().enumerate() {
        if width <= 0.0 {
            continue;
        }
        let cell = Rect::from_min_size(pos2(x, rect.min.y), vec2(width, rect.height()));
        x += width;
        let response = ui.interact(cell, ui.scope_id().with(("header", index)), Sense::click());
        let active = app.folders.sort == column && index != 1;
        let color = if active || response.hovered() {
            theme.fg
        } else {
            theme.dim
        };
        let arrow = if active {
            if app.folders.descending {
                " \u{2193}"
            } else {
                " \u{2191}"
            }
        } else {
            ""
        };
        let text = format!("{label}{arrow}");
        if right {
            ui.painter().text(
                pos2(cell.max.x - 10.0, cell.center().y),
                Align2::RIGHT_CENTER,
                text,
                theme.medium(11.5),
                color,
            );
        } else {
            ui.painter().text(
                pos2(cell.min.x + 12.0, cell.center().y),
                Align2::LEFT_CENTER,
                text,
                theme.medium(11.5),
                color,
            );
        }
        if response.clicked() {
            if app.folders.sort == column {
                app.folders.descending = !app.folders.descending;
            } else {
                app.folders.sort = column;
                app.folders.descending = column != Column::Name;
            }
            app.folders.dirty = true;
        }
    }
}

pub fn node_icon(theme: &Theme, tree: &Tree, id: NodeId, open: bool) -> (&'static str, Color32) {
    let node = tree.node(id);
    let flags = node.flags;
    if flags.has(NodeFlags::ERROR) {
        return (icons::WARNING, theme.attention);
    }
    if flags.has(NodeFlags::MOUNT) {
        return (icons::HARD_DRIVE, theme.faint);
    }
    if flags.has(NodeFlags::SYMLINK) {
        return (icons::LINK, theme.faint);
    }
    if node.is_dir() {
        return (if open { icons::FOLDER_OPEN } else { icons::FOLDER }, theme.accent);
    }
    let color = tree
        .exts
        .stats
        .get(node.ext as usize)
        .map(|s| theme.rgb(s.color))
        .unwrap_or(theme.dim);
    (icons::FILE, color)
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(shared) = app.shared() else { return };
    let theme = app.theme.clone();
    {
        let tree = shared.read();
        let scanning = app.is_scanning();
        let stale = tree.generation != app.folders.built_generation
            && app.folders.built.elapsed() > Duration::from_millis(if scanning { 600 } else { 0 });
        if app.folders.dirty || stale || (scanning && app.folders.built.elapsed() > Duration::from_millis(600)) {
            rebuild_rows(app, &tree);
        }
    }
    let full = ui.available_rect_before_wrap();
    let columns = Columns::fit(full.width());
    let header_rect = Rect::from_min_size(full.min, vec2(full.width(), HEADER_HEIGHT));
    ui.allocate_rect(header_rect, Sense::hover());
    header(ui, app, &columns, header_rect);
    let mut scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("folders-scroll");
    if let Some(target) = app.folders.scroll_to.take()
        && let Some(index) = app.folders.rows.iter().position(|r| r.node == target)
    {
        let view = ui.available_height();
        let offset = (index as f32 * ROW_HEIGHT - view / 2.0).max(0.0);
        let current = ui
            .ctx()
            .memory(|m| m.data.get_temp::<f32>(egui::Id::unique("folders-offset")))
            .unwrap_or(0.0);
        let row_top = index as f32 * ROW_HEIGHT;
        if row_top < current || row_top + ROW_HEIGHT > current + view - 4.0 {
            scroll = scroll.vertical_scroll_offset(offset);
        }
    }
    let tree = shared.read();
    let metric = app.settings.metric;
    let row_count = app.folders.rows.len();
    let mut clicked_row: Option<NodeId> = None;
    let mut toggled: Option<(NodeId, bool)> = None;
    let mut activated: Option<NodeId> = None;
    let output = scroll.show_rows(ui, ROW_HEIGHT, row_count, |ui, range| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for index in range {
            let row = app.folders.rows[index];
            let Some(node) = tree.get(row.node) else {
                ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
                continue;
            };
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
            let painter = ui.painter();
            let selected = app.selected == Some(row.node);
            if selected {
                painter.rect_filled(rect, 0, theme.select);
            } else if response.hovered() {
                painter.rect_filled(rect, 0, theme.hover.gamma_multiply(0.6));
            }
            if response.hovered() {
                app.hovered = Some(row.node);
            }
            if app.zoom == row.node && row.node != 0 {
                painter.vline(rect.min.x + 1.0, rect.y_range(), Stroke::new(2.0, theme.accent));
            }
            let open = app.folders.expanded.contains(&row.node);
            let mut x = rect.min.x + 8.0 + row.depth as f32 * 16.0;
            let has_children = node.is_dir() && node.child_count > 0;
            if has_children {
                let caret_rect = Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(16.0, ROW_HEIGHT));
                let caret = ui.interact(caret_rect, ui.scope_id().with(("caret", row.node)), Sense::click());
                painter.text(
                    caret_rect.center(),
                    Align2::CENTER_CENTER,
                    if open { icons::CARET_DOWN } else { icons::CARET_RIGHT },
                    theme.icon(11.0),
                    if caret.hovered() { theme.fg } else { theme.faint },
                );
                if caret.clicked() {
                    toggled = Some((row.node, !open));
                }
            }
            x += 16.0;
            let (icon, icon_color) = node_icon(&theme, &tree, row.node, open && has_children);
            painter.text(
                pos2(x + 7.0, rect.center().y),
                Align2::CENTER_CENTER,
                icon,
                theme.icon(14.0),
                icon_color,
            );
            x += 20.0;
            let name_right = rect.min.x + columns.name - 6.0;
            let name = tree.display_name(row.node);
            let dimmed = node.flags.has(NodeFlags::HARDLINK) || node.flags.has(NodeFlags::MOUNT);
            let fitted = widgets::fit_text(ui, &name, theme.ui(12.5), (name_right - x).max(10.0));
            painter.text(
                pos2(x, rect.center().y),
                Align2::LEFT_CENTER,
                fitted,
                theme.ui(12.5),
                if dimmed { theme.faint } else { theme.fg },
            );
            let weight = node.weight(metric);
            let parent_weight = if node.parent == u32::MAX {
                weight
            } else {
                tree.node(node.parent).weight(metric)
            };
            let fraction = if parent_weight == 0 {
                0.0
            } else {
                weight as f64 / parent_weight as f64
            };
            let mut cx = rect.min.x + columns.name;
            if columns.bar > 0.0 {
                let bar_rect =
                    Rect::from_min_size(pos2(cx + 4.0, rect.center().y - 4.0), vec2(columns.bar - 12.0, 8.0));
                widgets::bar(ui, &theme, bar_rect, fraction as f32, theme.accent.gamma_multiply(0.85));
                cx += columns.bar;
            }
            let painter = ui.painter();
            let cell = |painter: &egui::Painter, cx: &mut f32, width: f32, text: String, color: Color32| {
                if width <= 0.0 {
                    return;
                }
                *cx += width;
                painter.text(
                    pos2(*cx - 10.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    text,
                    theme.mono(11.5),
                    color,
                );
            };
            cell(
                painter,
                &mut cx,
                columns.percent,
                format!("{:.1}%", fraction * 100.0),
                theme.dim,
            );
            cell(painter, &mut cx, columns.size, human_size(weight), theme.fg);
            cell(
                painter,
                &mut cx,
                columns.other,
                human_size(node.weight(metric.other())),
                theme.dim,
            );
            let items = if node.is_dir() {
                human_count(node.items())
            } else {
                String::new()
            };
            cell(painter, &mut cx, columns.items, items, theme.dim);
            let files = if node.is_dir() {
                human_count(node.files as u64)
            } else {
                String::new()
            };
            cell(painter, &mut cx, columns.files, files, theme.dim);
            cell(painter, &mut cx, columns.modified, human_date(node.mtime), theme.dim);
            if response.clicked() {
                clicked_row = Some(row.node);
            }
            if response.double_clicked() {
                activated = Some(row.node);
            }
            if response.secondary_clicked() {
                clicked_row = Some(row.node);
            }
            if response.middle_clicked() {
                app.actions.push(Action::Open(row.node));
            }
            response.context_menu(|ui| actions::context_menu(app, &tree, ui, row.node));
        }
    });
    ui.ctx().memory_mut(|m| {
        m.data
            .insert_temp(egui::Id::unique("folders-offset"), output.state.clamped_offset().y)
    });
    drop(tree);
    if let Some((node, open)) = toggled {
        app.actions.push(Action::Expand(node, open));
    } else if let Some(node) = clicked_row {
        app.selected = Some(node);
    }
    if let Some(node) = activated {
        let is_dir = shared.read().get(node).is_some_and(|n| n.is_dir());
        if is_dir {
            let open = app.folders.expanded.contains(&node);
            app.actions.push(Action::Expand(node, !open));
        } else {
            app.actions.push(Action::Open(node));
        }
    }
}
