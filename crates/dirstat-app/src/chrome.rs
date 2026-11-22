use std::sync::atomic::Ordering;

use dirstat_core::disks;
use dirstat_core::format::{human_count, human_duration, human_size, short_count};
use dirstat_core::{Metric, ScanState};
use egui::{Align, Align2, Context, Frame, Key, Layout, Margin, RichText, ScrollArea, Sense, Stroke, Ui, vec2};

use crate::app::{Action, App, Dialog, Section, Tone};
use crate::icons;
use crate::settings::{Appearance, Arrangement};
use crate::widgets::{self, danger_button, primary_button, small, status_item, toggle_icon};

pub const TOP_HEIGHT: f32 = 40.0;

pub fn top_bar(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    let strip = ui.max_rect();
    ui.painter()
        .hline(strip.x_range(), strip.max.y - 0.5, Stroke::new(1.0, theme.line));
    ui.horizontal_centered(|ui| {
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        if !app.has_tree() {
            ui.label(
                RichText::new(format!("{}  dirstat", icons::TREEMAP))
                    .font(theme.semibold(13.5))
                    .color(theme.fg),
            );
        } else {
            breadcrumbs(app, ui, strip.width() - 470.0);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::icon_button(ui, &theme, icons::GEAR, "Settings (Ctrl+,)").clicked() {
                open_settings(app);
            }
            if !app.has_tree() {
                return;
            }
            let stacked = app.settings.arrangement == Arrangement::Stacked;
            if toggle_icon(
                ui,
                &theme,
                if stacked { icons::ROWS } else { icons::COLUMNS },
                false,
                "Switch between side by side and stacked (Ctrl+\\)",
            )
            .clicked()
            {
                toggle_arrangement(app);
            }
            if toggle_icon(
                ui,
                &theme,
                icons::TREEMAP,
                app.settings.show_treemap,
                "Treemap (Ctrl+M)",
            )
            .clicked()
            {
                app.settings.show_treemap = !app.settings.show_treemap;
                app.save_settings_soon();
            }
            if toggle_icon(ui, &theme, icons::SIDEBAR, app.settings.show_sidebar, "Lists (Ctrl+B)").clicked() {
                app.settings.show_sidebar = !app.settings.show_sidebar;
                app.save_settings_soon();
            }
            ui.add_space(10.0);
            let mut metric = app.settings.metric;
            widgets::chip(ui, &theme, &mut metric, Metric::Apparent, "Apparent");
            widgets::chip(ui, &theme, &mut metric, Metric::Allocated, "On disk");
            if metric != app.settings.metric {
                app.set_metric(metric);
            }
            ui.add_space(10.0);
            if app.is_scanning() {
                if ui
                    .button(RichText::new(format!("{}  Stop", icons::STOP)).color(theme.danger))
                    .on_hover_text("Esc")
                    .clicked()
                {
                    app.stop();
                }
            } else if ui
                .button(format!("{}  Rescan", icons::REFRESH))
                .on_hover_text("F5")
                .clicked()
            {
                app.rescan_all();
            }
            if ui
                .button(format!("{}  Open", icons::FOLDER_OPEN))
                .on_hover_text("Choose a folder (Ctrl+O)")
                .clicked()
            {
                app.choose_folder();
            }
        });
    });
}

pub fn toggle_arrangement(app: &mut App) {
    app.settings.arrangement = match app.settings.arrangement {
        Arrangement::SideBySide => Arrangement::Stacked,
        Arrangement::Stacked => Arrangement::SideBySide,
    };
    app.save_settings_soon();
}

pub fn open_settings(app: &mut App) {
    app.dialog = Some(Dialog::Settings {
        excludes: app.settings.excludes.join("\n"),
    });
}

fn breadcrumbs(app: &mut App, ui: &mut Ui, max_width: f32) {
    let theme = app.theme.clone();
    let Some(tree) = app.shared() else { return };
    let chain: Vec<(u32, String)> = {
        let tree = tree.read();
        tree.ancestors(app.zoom)
            .into_iter()
            .map(|id| (id, tree.display_name(id)))
            .collect()
    };
    let mut widths: Vec<f32> = chain
        .iter()
        .map(|(_, name)| {
            ui.painter()
                .layout_no_wrap(name.clone(), theme.ui(12.5), theme.fg)
                .size()
                .x
                + 26.0
        })
        .collect();
    let mut skip = 0;
    while widths.iter().sum::<f32>() > max_width.max(120.0) && skip + 1 < chain.len() {
        widths[skip] = 0.0;
        skip += 1;
    }
    if app.zoom != 0 && widgets::icon_button(ui, &theme, icons::ARROW_ELBOW_LEFT_UP, "Zoom out (Backspace)").clicked() {
        app.actions.push(Action::ZoomOut);
    }
    if skip > 0 {
        ui.label(RichText::new("...").color(theme.faint));
        ui.label(
            RichText::new(icons::CARET_RIGHT)
                .font(theme.icon(10.0))
                .color(theme.faint),
        );
    }
    let last = chain.len() - 1;
    for (index, (id, name)) in chain.iter().enumerate().skip(skip) {
        let current = index == last;
        let text = if index == 0 {
            RichText::new(format!("{}  {name}", icons::HARD_DRIVE))
        } else {
            RichText::new(name.clone())
        };
        let text = if current {
            text.font(theme.medium(12.5)).color(theme.fg)
        } else {
            text.font(theme.ui(12.5)).color(theme.dim)
        };
        let response = ui.add(egui::Button::new(text).frame(false).min_size(vec2(0.0, 26.0)));
        if response.clicked() && !current {
            app.actions.push(Action::Zoom(*id));
        }
        if !current {
            ui.label(
                RichText::new(icons::CARET_RIGHT)
                    .font(theme.icon(10.0))
                    .color(theme.faint),
            );
        }
    }
}

pub fn status_bar(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if let Some(job) = app.job.clone() {
            if job.is_running() {
                widgets::spinner(ui, &theme);
                let stage = if job.state() == ScanState::Finalizing {
                    "Sorting".to_string()
                } else {
                    format!(
                        "{} items  {}  {}",
                        short_count(job.entries()),
                        human_size(job.alloc.load(Ordering::Relaxed)),
                        human_duration(job.elapsed())
                    )
                };
                ui.label(RichText::new(stage).font(theme.mono(11.0)).color(theme.working));
                let current = job.current().display().to_string();
                ui.add_space(6.0);
                ui.label(
                    RichText::new(widgets::truncate_middle(&current, 70))
                        .size(11.0)
                        .color(theme.faint),
                );
            } else if let Some(summary) = app.status_summary() {
                let took = match job.state() {
                    ScanState::Cancelled => "  stopped".to_string(),
                    _ => format!("  in {}", human_duration(job.elapsed())),
                };
                ui.label(RichText::new(format!("{summary}{took}")).size(11.5).color(theme.dim));
            }
            let errors = app.shared().map(|t| t.read().errors.len() as u64).unwrap_or(0);
            if errors > 0
                && status_item(
                    ui,
                    &theme,
                    icons::WARNING,
                    &format!("{} unreadable", human_count(errors)),
                    theme.attention,
                )
                .on_hover_text("Folders that could not be read")
                .clicked()
            {
                app.dialog = Some(Dialog::Errors);
            }
        } else if let Some(summary) = app.status_summary() {
            ui.label(RichText::new(summary).size(11.5).color(theme.dim));
        }
        let focus = app.last_hovered.or(app.selected);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(2.0);
            if status_item(ui, &theme, icons::SEARCH, "Ctrl+K", theme.faint)
                .on_hover_text("Commands and quick find")
                .clicked()
            {
                app.palette = Some(crate::palette::Palette::new());
            }
            let (icon, next) = match app.settings.appearance {
                Appearance::System => (icons::CIRCLE, Appearance::Dark),
                Appearance::Dark => (icons::MOON, Appearance::Light),
                Appearance::Light => (icons::SUN, Appearance::System),
            };
            if status_item(ui, &theme, icon, "", theme.dim)
                .on_hover_text(format!("Theme: {:?}", app.settings.appearance))
                .clicked()
            {
                app.settings.appearance = next;
                app.save_settings_soon();
            }
            if let Some(root) = app.root_path()
                && let Some(disk) = disks::containing(&app.disks, &root)
            {
                ui.label(
                    RichText::new(format!("{} free of {}", human_size(disk.free), human_size(disk.total)))
                        .size(11.5)
                        .color(theme.faint),
                );
                ui.add_space(8.0);
            }
            if let Some(node) = focus
                && let Some(tree) = app.shared()
            {
                let tree = tree.read();
                if tree.get(node).is_some() {
                    let path = tree.path(node).display().to_string();
                    let size = human_size(tree.weight(node, app.settings.metric));
                    let room = (ui.available_width() / 6.5) as usize;
                    ui.label(RichText::new(size).font(theme.mono(11.0)).color(theme.fg));
                    ui.label(
                        RichText::new(widgets::truncate_middle(&path, room.saturating_sub(14)))
                            .size(11.5)
                            .color(theme.dim),
                    );
                }
            }
        });
    });
}

pub fn toasts(app: &mut App, ctx: &Context) {
    if app.toasts.is_empty() {
        return;
    }
    let theme = app.theme.clone();
    let area = egui::Area::new(egui::Id::unique("dirstat-toasts"))
        .order(egui::Order::Foreground)
        .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -34.0))
        .show(ctx, |ui| {
            for toast in &app.toasts {
                let color = match toast.tone {
                    Tone::Info => theme.working,
                    Tone::Good => theme.ok,
                    Tone::Bad => theme.danger,
                };
                Frame::new()
                    .fill(theme.raised)
                    .stroke(Stroke::new(1.0, theme.line))
                    .corner_radius(8)
                    .shadow(ui.visuals().popup_shadow)
                    .inner_margin(Margin::symmetric(14, 10))
                    .outer_margin(Margin::symmetric(0, 4))
                    .show(ui, |ui| {
                        ui.set_max_width(440.0);
                        ui.horizontal(|ui| {
                            let icon = match toast.tone {
                                Tone::Info => icons::CIRCLE,
                                Tone::Good => icons::CHECK_CIRCLE,
                                Tone::Bad => icons::X_CIRCLE,
                            };
                            ui.label(RichText::new(icon).font(theme.icon(15.0)).color(color));
                            ui.add(egui::Label::new(RichText::new(&toast.text).size(12.5).color(theme.fg)).wrap());
                        });
                    });
            }
        });
    if area.response.contains_pointer() {
        let hold = std::time::Instant::now() + std::time::Duration::from_secs(2);
        for toast in &mut app.toasts {
            toast.until = toast.until.max(hold);
        }
    }
}

pub fn dialogs(app: &mut App, ctx: &Context) {
    let Some(dialog) = app.dialog.take() else { return };
    let theme = app.theme.clone();
    let mut keep = true;
    let next = match dialog {
        Dialog::Remove {
            nodes,
            paths,
            bytes,
            permanent,
        } => {
            let mut confirm = false;
            let modal = egui::Modal::new(egui::Id::unique("confirm-remove")).show(ctx, |ui| {
                ui.set_width(480.0);
                let count = paths.len();
                let what = if count == 1 {
                    "this item".to_string()
                } else {
                    format!("these {count} items")
                };
                let title = if permanent {
                    format!("Delete {what} permanently?")
                } else {
                    format!("Move {what} to the trash?")
                };
                ui.label(widgets::heading(&theme, title));
                ui.add_space(4.0);
                let detail = if permanent {
                    format!("{} will be freed. This cannot be undone.", human_size(bytes))
                } else {
                    format!("{} will be freed once the trash is emptied.", human_size(bytes))
                };
                ui.label(small(&theme, detail));
                ui.add_space(8.0);
                Frame::new()
                    .fill(theme.field)
                    .stroke(Stroke::new(1.0, theme.line))
                    .corner_radius(6)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for path in paths.iter().take(6) {
                            ui.label(
                                RichText::new(widgets::truncate_middle(&path.display().to_string(), 64))
                                    .font(theme.mono(11.5))
                                    .color(theme.dim),
                            );
                        }
                        if paths.len() > 6 {
                            ui.label(small(&theme, format!("and {} more", paths.len() - 6)));
                        }
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let button = if permanent {
                            danger_button(ui, &theme, "Delete")
                        } else {
                            primary_button(ui, &theme, "Move to trash")
                        };
                        if button.clicked() {
                            confirm = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    confirm = true;
                }
            });
            if modal.should_close() {
                keep = false;
            }
            if confirm {
                app.perform_remove(nodes, paths, bytes, permanent);
                return;
            }
            Dialog::Remove {
                nodes,
                paths,
                bytes,
                permanent,
            }
        }
        Dialog::Errors => {
            let modal = egui::Modal::new(egui::Id::unique("errors")).show(ctx, |ui| {
                ui.set_width(560.0);
                ui.label(widgets::heading(&theme, "Folders that could not be read"));
                ui.add_space(4.0);
                ui.label(small(
                    &theme,
                    "Their size is missing from the totals. Running as an administrator may help.",
                ));
                ui.add_space(8.0);
                let entries: Vec<(u32, String, String)> = app
                    .shared()
                    .map(|tree| {
                        let tree = tree.read();
                        tree.errors
                            .iter()
                            .filter(|(id, _)| tree.get(*id).is_some())
                            .take(500)
                            .map(|(id, kind)| (*id, tree.path(*id).display().to_string(), format!("{kind}")))
                            .collect()
                    })
                    .unwrap_or_default();
                ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                    for (id, path, kind) in &entries {
                        ui.horizontal(|ui| {
                            let label = ui.add(
                                egui::Label::new(
                                    RichText::new(widgets::truncate_middle(path, 60))
                                        .font(theme.mono(11.5))
                                        .color(theme.fg),
                                )
                                .sense(Sense::click()),
                            );
                            if label.clicked() {
                                app.actions.push(Action::Select(*id));
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(small(&theme, kind));
                            });
                        });
                    }
                    if entries.is_empty() {
                        ui.label(small(&theme, "Nothing to show"));
                    }
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                });
            });
            if modal.should_close() {
                keep = false;
            }
            Dialog::Errors
        }
        Dialog::Keys => {
            let modal = egui::Modal::new(egui::Id::unique("keys")).show(ctx, |ui| {
                ui.set_width(560.0);
                ui.label(widgets::heading(&theme, "Keyboard shortcuts"));
                ui.add_space(8.0);
                ui.columns(2, |columns| {
                    widgets::key_hints(&mut columns[0], &theme, KEYS_LEFT, 250.0);
                    widgets::key_hints(&mut columns[1], &theme, KEYS_RIGHT, 250.0);
                });
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        keep = false;
                    }
                });
            });
            if modal.should_close() {
                keep = false;
            }
            Dialog::Keys
        }
        Dialog::Settings { mut excludes } => {
            let mut changed = false;
            let modal = egui::Modal::new(egui::Id::unique("settings")).show(ctx, |ui| {
                ui.set_width(520.0);
                ui.spacing_mut().slider_width = 220.0;
                ui.label(widgets::heading(&theme, "Settings"));
                ui.add_space(10.0);
                let settings = &mut app.settings;
                let row = |ui: &mut Ui, label: &str, add: &mut dyn FnMut(&mut Ui)| {
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(vec2(160.0, 26.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_width(160.0);
                            ui.label(RichText::new(label).size(12.5).color(theme.dim));
                        });
                        add(ui);
                    });
                };
                row(ui, "Theme", &mut |ui| {
                    changed |= widgets::chip(ui, &theme, &mut settings.appearance, Appearance::System, "System");
                    changed |= widgets::chip(ui, &theme, &mut settings.appearance, Appearance::Dark, "Dark");
                    changed |= widgets::chip(ui, &theme, &mut settings.appearance, Appearance::Light, "Light");
                });
                row(ui, "Treemap shading", &mut |ui| {
                    changed |= widgets::chip(ui, &theme, &mut settings.cushion, true, "Cushions");
                    changed |= widgets::chip(ui, &theme, &mut settings.cushion, false, "Flat");
                });
                row(ui, "Brightness", &mut |ui| {
                    changed |= ui
                        .add(egui::Slider::new(&mut settings.brightness, 0.6..=1.4).show_value(false))
                        .changed();
                });
                row(ui, "Cushion height", &mut |ui| {
                    changed |= ui
                        .add(egui::Slider::new(&mut settings.cushion_height, 0.1..=0.8).show_value(false))
                        .changed();
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(widgets::heading(&theme, "Scanning"));
                ui.add_space(4.0);
                changed |= ui
                    .checkbox(
                        &mut settings.cross_filesystems,
                        "Cross into other file systems and mounted drives",
                    )
                    .changed();
                changed |= ui
                    .checkbox(&mut settings.dedup_hardlinks, "Count hard-linked files once")
                    .changed();
                changed |= ui
                    .checkbox(&mut settings.confirm_trash, "Ask before moving to the trash")
                    .changed();
                row(ui, "Threads", &mut |ui| {
                    let mut threads = settings.threads as u32;
                    let response = ui.add(
                        egui::DragValue::new(&mut threads)
                            .range(0..=128)
                            .custom_formatter(|v, _| if v == 0.0 { "Auto".into() } else { format!("{v}") }),
                    );
                    if response.changed() {
                        settings.threads = threads as usize;
                        changed = true;
                    }
                });
                ui.add_space(6.0);
                ui.label(small(
                    &theme,
                    "Excluded names and paths, one per line. Use * and ?, or an absolute path.",
                ));
                let response = ui.add(
                    egui::TextEdit::multiline(&mut excludes)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .font(theme.mono(12.0))
                        .hint_text("node_modules\n*.tmp\n/home/me/.cache"),
                );
                if response.changed() {
                    settings.excludes = excludes
                        .lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect();
                    changed = true;
                }
                ui.add_space(4.0);
                ui.label(small(
                    &theme,
                    format!("Saved in {}", crate::settings::Settings::path().display()),
                ));
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary_button(ui, &theme, "Done").clicked() {
                        keep = false;
                    }
                });
            });
            if modal.should_close() {
                keep = false;
            }
            if changed {
                app.save_settings_soon();
            }
            Dialog::Settings { excludes }
        }
    };
    if keep && app.dialog.is_none() {
        app.dialog = Some(next);
    }
}

pub const KEYS_LEFT: &[(&str, &str)] = &[
    ("Ctrl+K", "Commands and quick find"),
    ("Ctrl+O", "Scan a folder"),
    ("F5", "Rescan everything"),
    ("Shift+F5", "Rescan the selection"),
    ("Esc", "Stop the scan"),
    ("Ctrl+F", "Search"),
    ("Ctrl+B", "Show or hide the lists"),
    ("Ctrl+M", "Show or hide the treemap"),
    ("Ctrl+\\", "Side by side or stacked"),
    ("Alt+1..5", "Folders, types, largest, duplicates, search"),
];

pub const KEYS_RIGHT: &[(&str, &str)] = &[
    ("Enter", "Zoom into the selection"),
    ("Backspace", "Zoom out"),
    ("Up / Down", "Move the selection"),
    ("Left / Right", "Collapse or expand"),
    ("Ctrl+Enter", "Open"),
    ("Ctrl+Shift+R", "Show in file manager"),
    ("Ctrl+Shift+C", "Copy the path"),
    ("Delete", "Move to the trash"),
    ("Shift+Delete", "Delete permanently"),
    ("Ctrl+E", "Export"),
];

pub fn section_label(section: Section) -> &'static str {
    match section {
        Section::Folders => "Folders",
        Section::Types => "Types",
        Section::Largest => "Largest",
        Section::Duplicates => "Duplicates",
        Section::Search => "Search",
    }
}
