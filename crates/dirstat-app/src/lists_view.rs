use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use dirstat_core::dupes::DupProgress;
use dirstat_core::format::{human_count, human_date, human_duration, human_size};
use dirstat_core::largest::largest_files;
use dirstat_core::search::{Query, search};
use dirstat_core::{NodeId, Tree};
use egui::{Align2, Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::actions;
use crate::app::{Action, App};
use crate::icons;
use crate::tree_view::node_icon;
use crate::widgets;

const ROW: f32 = 34.0;

pub fn types(app: &mut App, ui: &mut Ui) {
    let Some(shared) = app.shared() else { return };
    let theme = app.theme.clone();
    let metric = app.settings.metric;
    let tree = shared.read();
    let ranked = tree.exts.ranked(metric);
    let total: u64 = ranked
        .iter()
        .map(|&id| tree.exts.stats[id as usize].weight(metric))
        .sum();
    let header = ui
        .allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover())
        .0;
    let painter = ui.painter();
    painter.rect_filled(header, 0, theme.panel);
    painter.hline(header.x_range(), header.max.y - 0.5, Stroke::new(1.0, theme.line));
    let width = header.width();
    let size_x = header.max.x - 10.0;
    let files_x = size_x - 96.0;
    let percent_x = files_x - 84.0;
    for (x, label, align) in [
        (header.min.x + 40.0, "Extension", Align2::LEFT_CENTER),
        (percent_x, "%", Align2::RIGHT_CENTER),
        (files_x, "Files", Align2::RIGHT_CENTER),
        (size_x, metric.label(), Align2::RIGHT_CENTER),
    ] {
        painter.text(pos2(x, header.center().y), align, label, theme.medium(11.5), theme.dim);
    }
    let mut chosen: Option<Action> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("types-scroll")
        .show_rows(ui, 26.0, ranked.len(), |ui, range| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for &id in &ranked[range] {
                let stat = &tree.exts.stats[id as usize];
                let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
                let painter = ui.painter();
                let active = app.highlight_ext == Some(id);
                if active {
                    painter.rect_filled(rect, 0, theme.select);
                } else if response.hovered() {
                    painter.rect_filled(rect, 0, theme.hover.gamma_multiply(0.6));
                }
                let swatch = Rect::from_center_size(pos2(rect.min.x + 22.0, rect.center().y), vec2(14.0, 14.0));
                painter.rect_filled(swatch, 3, theme.rgb(stat.color));
                let name = if stat.name.is_empty() {
                    "(no extension)".to_string()
                } else {
                    format!(".{}", stat.name)
                };
                painter.text(
                    pos2(rect.min.x + 40.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    theme.mono(12.0),
                    theme.fg,
                );
                let weight = stat.weight(metric);
                let share = if total == 0 { 0.0 } else { weight as f32 / total as f32 };
                let bar = Rect::from_min_size(pos2(percent_x - 120.0, rect.center().y - 3.0), vec2(60.0, 6.0));
                if bar.min.x > rect.min.x + 150.0 {
                    widgets::bar(ui, &theme, bar, share, theme.rgb(stat.color));
                }
                let painter = ui.painter();
                painter.text(
                    pos2(percent_x, rect.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{:.1}%", share * 100.0),
                    theme.mono(11.5),
                    theme.dim,
                );
                painter.text(
                    pos2(files_x, rect.center().y),
                    Align2::RIGHT_CENTER,
                    human_count(stat.files),
                    theme.mono(11.5),
                    theme.dim,
                );
                painter.text(
                    pos2(size_x, rect.center().y),
                    Align2::RIGHT_CENTER,
                    human_size(weight),
                    theme.mono(11.5),
                    theme.fg,
                );
                if response.clicked() {
                    chosen = Some(Action::Highlight(Some(id)));
                }
                if response.double_clicked() && !stat.name.is_empty() {
                    chosen = Some(Action::SearchFor(format!("ext:{}", stat.name)));
                }
                response.on_hover_text("Click to highlight in the treemap, double-click to list the files");
            }
        });
    drop(tree);
    if let Some(action) = chosen {
        app.actions.push(action);
    }
}

pub fn node_list(app: &mut App, ui: &mut Ui, tree: &Tree, items: &[NodeId], salt: &str) {
    let theme = app.theme.clone();
    let metric = app.settings.metric;
    let width = ui.available_width();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt(salt)
        .show_rows(ui, ROW, items.len(), |ui, range| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for &id in &items[range] {
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                let Some(node) = tree.get(id) else { continue };
                let painter = ui.painter();
                if app.is_selected(id) {
                    painter.rect_filled(rect, 0, theme.select);
                } else if response.hovered() {
                    painter.rect_filled(rect, 0, theme.hover.gamma_multiply(0.6));
                }
                if response.hovered() {
                    app.hovered = Some(id);
                }
                let (icon, color) = node_icon(&theme, tree, id, false);
                painter.text(
                    pos2(rect.min.x + 18.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    icon,
                    theme.icon(15.0),
                    color,
                );
                let right_width = 190.0;
                let text_width = (rect.width() - 40.0 - right_width).max(40.0);
                let name = widgets::fit_text(ui, &tree.display_name(id), theme.ui(12.5), text_width);
                let parent = if node.parent == u32::MAX {
                    String::new()
                } else {
                    tree.path(node.parent).display().to_string()
                };
                let parent = widgets::fit_middle(ui, &parent, theme.ui(11.0), text_width);
                let painter = ui.painter();
                painter.text(
                    pos2(rect.min.x + 36.0, rect.min.y + 11.0),
                    Align2::LEFT_CENTER,
                    name,
                    theme.ui(12.5),
                    theme.fg,
                );
                painter.text(
                    pos2(rect.min.x + 36.0, rect.min.y + 25.0),
                    Align2::LEFT_CENTER,
                    parent,
                    theme.ui(11.0),
                    theme.faint,
                );
                painter.text(
                    pos2(rect.max.x - 10.0, rect.min.y + 11.0),
                    Align2::RIGHT_CENTER,
                    human_size(node.weight(metric)),
                    theme.mono(12.0),
                    theme.fg,
                );
                painter.text(
                    pos2(rect.max.x - 10.0, rect.min.y + 25.0),
                    Align2::RIGHT_CENTER,
                    human_date(node.mtime),
                    theme.mono(10.5),
                    theme.faint,
                );
                if response.double_clicked() {
                    app.actions.push(Action::Open(id));
                } else if response.clicked() {
                    let modifiers = ui.input(|i| i.modifiers);
                    if !app.pick(id, modifiers, items) {
                        app.actions.push(Action::Select(id));
                    }
                } else if response.secondary_clicked() && !app.is_selected(id) {
                    app.actions.push(Action::Select(id));
                }
                response.context_menu(|ui| actions::context_menu(app, tree, ui, id));
            }
        });
}

fn scope_line(app: &App, ui: &mut Ui, tree: &Tree, text: String) {
    let theme = &app.theme;
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(widgets::small(theme, text));
        if app.zoom != tree.root {
            ui.label(
                RichText::new(format!("in {}", tree.display_name(app.zoom)))
                    .size(12.0)
                    .color(theme.accent),
            );
        }
    });
    ui.add_space(4.0);
}

pub fn largest(app: &mut App, ui: &mut Ui) {
    let Some(shared) = app.shared() else { return };
    let scanning = app.is_scanning();
    let tree = shared.read();
    let key = (tree.generation, app.zoom, app.settings.metric, scanning);
    if app.largest.key != Some(key) {
        app.largest.items = largest_files(&tree, app.zoom, app.settings.metric, 1000);
        app.largest.key = Some(key);
    }
    ui.add_space(8.0);
    let count = app.largest.items.len();
    scope_line(app, ui, &tree, format!("The {count} largest files"));
    let items = std::mem::take(&mut app.largest.items);
    node_list(app, ui, &tree, &items, "largest-scroll");
    app.largest.items = items;
}

pub fn search_view(app: &mut App, ui: &mut Ui) {
    let Some(shared) = app.shared() else { return };
    let theme = app.theme.clone();
    ui.add_space(8.0);
    let response = ui
        .horizontal(|ui| {
            ui.add_space(10.0);
            ui.add(
                egui::TextEdit::singleline(&mut app.search.text)
                    .hint_text("Name, ext:jpg,png  size:>100mb  older:1y  newer:7d  kind:dir")
                    .desired_width(ui.available_width() - 10.0)
                    .font(theme.ui(13.0)),
            )
        })
        .inner;
    if app.search.focus {
        response.request_focus();
        app.search.focus = false;
    }
    if response.changed() {
        app.search.edited = Some(Instant::now());
    }
    let tree = shared.read();
    let wanted = (app.search.text.trim().to_string(), tree.generation, app.zoom);
    let waited = app
        .search
        .edited
        .is_none_or(|at| at.elapsed() > Duration::from_millis(140));
    if app.search.ran.as_ref() != Some(&wanted) && waited && !app.is_scanning() {
        let started = Instant::now();
        let query = Query::parse(&wanted.0);
        app.search.results = search(&tree, &query, app.zoom, app.settings.metric, 5000);
        app.search.took = started.elapsed();
        app.search.ran = Some(wanted);
    } else if !waited {
        ui.ctx().request_repaint_after(Duration::from_millis(150));
    }
    ui.add_space(6.0);
    if app.search.text.trim().is_empty() {
        widgets::empty_state(
            ui,
            &theme,
            icons::SEARCH,
            "Find files and folders",
            "Combine words with filters like ext:mp4 size:>1gb older:2y",
        );
        return;
    }
    if app.is_scanning() {
        ui.label(widgets::small(&theme, "   Search runs when the scan finishes"));
        return;
    }
    let count = app.search.results.len();
    let total: u64 = app
        .search
        .results
        .iter()
        .filter_map(|&id| tree.get(id))
        .map(|n| n.weight(app.settings.metric))
        .sum();
    let more = if count >= 5000 { "+" } else { "" };
    scope_line(
        app,
        ui,
        &tree,
        format!(
            "{}{more} matches, {} in {}",
            human_count(count as u64),
            human_size(total),
            human_duration(app.search.took)
        ),
    );
    let items = std::mem::take(&mut app.search.results);
    node_list(app, ui, &tree, &items, "search-scroll");
    app.search.results = items;
}

pub fn duplicates(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.add_space(8.0);
    if let Some(progress) = app.dupes.progress.clone() {
        let stage = match progress.stage.load(Ordering::Relaxed) {
            DupProgress::COLLECTING => "Grouping files by size",
            DupProgress::SAMPLING => "Comparing the start and end of each candidate",
            _ => "Hashing full contents",
        };
        let done = progress.files_done.load(Ordering::Relaxed);
        let total = progress.files_total.load(Ordering::Relaxed).max(1);
        let bytes = progress.bytes_done.load(Ordering::Relaxed);
        let bytes_total = progress.bytes_total.load(Ordering::Relaxed);
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            widgets::empty_heading(ui, &theme, icons::FINGERPRINT, "Looking for duplicates");
            ui.add_space(6.0);
            ui.label(widgets::small(&theme, stage));
            ui.add_space(10.0);
            let (rect, _) = ui.allocate_exact_size(vec2(280.0, 6.0), Sense::hover());
            let fraction = if bytes_total > 0 {
                bytes as f32 / bytes_total as f32
            } else {
                done as f32 / total as f32
            };
            widgets::bar(ui, &theme, rect, fraction, theme.working);
            ui.add_space(6.0);
            let detail = if bytes_total > 0 {
                format!(
                    "{} of {} files, {} of {}",
                    human_count(done),
                    human_count(total),
                    human_size(bytes),
                    human_size(bytes_total)
                )
            } else {
                format!("{} of {} files", human_count(done), human_count(total))
            };
            ui.label(RichText::new(detail).font(theme.mono(11.0)).color(theme.faint));
            ui.add_space(12.0);
            if ui.button("Cancel").clicked() {
                app.cancel_duplicates();
            }
        });
        return;
    }
    let mut min = app.settings.dupes_min_size;
    let mut run = false;
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(widgets::small(&theme, "Files of at least"));
        for (value, label) in [
            (0u64, "any size"),
            (1 << 20, "1 MB"),
            (10 << 20, "10 MB"),
            (100 << 20, "100 MB"),
        ] {
            widgets::chip(ui, &theme, &mut min, value, label);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            let label = if app.dupes.ran {
                "Search again"
            } else {
                "Find duplicates"
            };
            if widgets::primary_button(ui, &theme, label).clicked() {
                run = true;
            }
        });
    });
    if min != app.settings.dupes_min_size {
        app.settings.dupes_min_size = min;
        app.save_settings_soon();
    }
    if run {
        app.find_duplicates();
        return;
    }
    ui.add_space(6.0);
    if !app.dupes.ran {
        widgets::empty_state(
            ui,
            &theme,
            icons::FINGERPRINT,
            "Find files with the same content",
            "Files are matched by size, then by a sample, then by a full BLAKE3 hash",
        );
        return;
    }
    if app.dupes.groups.is_empty() {
        widgets::empty_state(ui, &theme, icons::CHECK_CIRCLE, "No duplicates", "Every file is unique");
        return;
    }
    let Some(shared) = app.shared() else { return };
    let tree = shared.read();
    let wasted: u64 = app.dupes.groups.iter().map(|g| g.wasted()).sum();
    let checked_bytes: u64 = app
        .dupes
        .checked
        .iter()
        .filter_map(|&id| tree.get(id))
        .map(|n| n.weight(app.settings.metric))
        .sum();
    let mut keep_newest = false;
    let mut keep_oldest = false;
    let mut clear = false;
    let mut trash = false;
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(widgets::small(
            &theme,
            format!(
                "{} groups, {} reclaimable, found in {}",
                human_count(app.dupes.groups.len() as u64),
                human_size(wasted),
                human_duration(app.dupes.took)
            ),
        ));
    });
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        if ui.button("Select all but newest").clicked() {
            keep_newest = true;
        }
        if ui.button("Select all but oldest").clicked() {
            keep_oldest = true;
        }
        if !app.dupes.checked.is_empty() && ui.button("Clear").clicked() {
            clear = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            let count = app.dupes.checked.len();
            let label = format!("Trash {count} ({})", human_size(checked_bytes));
            let button = ui.add_enabled_ui(count > 0, |ui| widgets::danger_button(ui, &theme, &label));
            if button.inner.clicked() {
                trash = true;
            }
        });
    });
    ui.add_space(4.0);
    if keep_newest || keep_oldest {
        app.dupes.checked.clear();
        for group in &app.dupes.groups {
            let live: Vec<NodeId> = group.nodes.iter().copied().filter(|&n| tree.get(n).is_some()).collect();
            let keep = if keep_newest {
                live.iter().copied().max_by_key(|&n| tree.node(n).mtime)
            } else {
                live.iter().copied().min_by_key(|&n| tree.node(n).mtime)
            };
            for node in live {
                if Some(node) != keep {
                    app.dupes.checked.insert(node);
                }
            }
        }
    }
    if clear {
        app.dupes.checked.clear();
    }
    if trash {
        let nodes: Vec<NodeId> = app.dupes.checked.iter().copied().collect();
        app.actions.push(Action::Trash(nodes));
    }
    let width = ui.available_width();
    let groups = std::mem::take(&mut app.dupes.groups);
    let mut toggles: Vec<(NodeId, bool)> = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("dupes-scroll")
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for (index, group) in groups.iter().enumerate() {
                let collapsed = app.dupes.collapsed.contains(&index);
                let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
                let painter = ui.painter();
                painter.rect_filled(rect, 0, theme.panel);
                painter.hline(rect.x_range(), rect.max.y - 0.5, Stroke::new(1.0, theme.line));
                painter.text(
                    pos2(rect.min.x + 14.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    if collapsed {
                        icons::CARET_RIGHT
                    } else {
                        icons::CARET_DOWN
                    },
                    theme.icon(11.0),
                    theme.faint,
                );
                let title = format!("{} copies of {}", group.nodes.len(), tree.name_lossy(group.nodes[0]));
                let title = widgets::fit_text(ui, &title, theme.medium(12.5), rect.width() - 200.0);
                let painter = ui.painter();
                painter.text(
                    pos2(rect.min.x + 28.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    title,
                    theme.medium(12.5),
                    theme.fg,
                );
                painter.text(
                    pos2(rect.max.x - 10.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{} each, {} wasted", human_size(group.size), human_size(group.wasted())),
                    theme.mono(11.0),
                    theme.attention,
                );
                if response.clicked() {
                    if collapsed {
                        app.dupes.collapsed.remove(&index);
                    } else {
                        app.dupes.collapsed.insert(index);
                    }
                }
                if collapsed {
                    continue;
                }
                for (&node, path) in group.nodes.iter().zip(group.paths.iter()) {
                    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
                    let Some(entry) = tree.get(node) else { continue };
                    let painter = ui.painter();
                    let checked = app.dupes.checked.contains(&node);
                    if app.is_selected(node) {
                        painter.rect_filled(rect, 0, theme.select);
                    } else if response.hovered() {
                        painter.rect_filled(rect, 0, theme.hover.gamma_multiply(0.6));
                    }
                    let check = Rect::from_center_size(pos2(rect.min.x + 30.0, rect.center().y), vec2(14.0, 14.0));
                    painter.rect_stroke(
                        check,
                        3,
                        Stroke::new(1.0, if checked { theme.danger } else { theme.faint }),
                        egui::StrokeKind::Inside,
                    );
                    if checked {
                        painter.rect_filled(check.shrink(1.0), 2, theme.danger);
                        painter.text(
                            check.center(),
                            Align2::CENTER_CENTER,
                            icons::CHECK,
                            theme.icon(10.0),
                            Color32::WHITE,
                        );
                    }
                    let shown = path.strip_prefix(&tree.root_path).unwrap_or(path).display().to_string();
                    let text = widgets::fit_middle(ui, &shown, theme.ui(12.0), rect.width() - 190.0);
                    let painter = ui.painter();
                    painter.text(
                        pos2(rect.min.x + 46.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        text,
                        theme.ui(12.0),
                        if checked { theme.dim } else { theme.fg },
                    );
                    painter.text(
                        pos2(rect.max.x - 10.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        human_date(entry.mtime),
                        theme.mono(10.5),
                        theme.faint,
                    );
                    let pointer_on_check = response.interact_pointer_pos().is_some_and(|p| p.x < rect.min.x + 44.0);
                    if response.clicked() && pointer_on_check {
                        app.actions.push(Action::Select(node));
                        toggles.push((node, !checked));
                    } else if response.double_clicked() {
                        app.actions.push(Action::Open(node));
                    } else if response.clicked() {
                        let modifiers = ui.input(|i| i.modifiers);
                        if !app.pick(node, modifiers, &group.nodes) {
                            app.actions.push(Action::Select(node));
                        }
                    } else if response.secondary_clicked() && !app.is_selected(node) {
                        app.actions.push(Action::Select(node));
                    }
                    response.context_menu(|ui| actions::context_menu(app, &tree, ui, node));
                }
            }
        });
    drop(tree);
    if app.dupes.groups.is_empty() {
        app.dupes.groups = groups;
    }
    for (node, on) in toggles {
        if on {
            app.dupes.checked.insert(node);
        } else {
            app.dupes.checked.remove(&node);
        }
    }
}
