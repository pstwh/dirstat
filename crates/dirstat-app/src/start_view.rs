use dirstat_core::format::human_size;
use egui::{Align2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};

use crate::app::App;
use crate::icons;
use crate::widgets;

pub fn show(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    let mut chosen = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.12).max(24.0));
            widgets::empty_heading(ui, &theme, icons::TREEMAP, "Pick something to scan");
            ui.add_space(4.0);
            ui.label(widgets::small(&theme, "See what takes up space, then clean it up"));
            ui.add_space(22.0);
            let card = vec2(250.0, 78.0);
            let gap = 12.0;
            let columns = (((ui.available_width() - 80.0) + gap) / (card.x + gap))
                .floor()
                .clamp(1.0, 4.0) as usize;
            let total_width = columns as f32 * card.x + (columns as f32 - 1.0) * gap;
            for chunk in app.disks.chunks(columns) {
                ui.allocate_ui(vec2(total_width, card.y), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        let used_width = chunk.len() as f32 * card.x + (chunk.len() as f32 - 1.0) * gap;
                        ui.add_space((total_width - used_width) / 2.0);
                        for disk in chunk {
                            let (rect, response) = ui.allocate_exact_size(card, Sense::click());
                            let painter = ui.painter();
                            painter.rect(
                                rect,
                                10,
                                if response.hovered() { theme.raised } else { theme.panel },
                                Stroke::new(1.0, if response.hovered() { theme.accent } else { theme.line }),
                                egui::StrokeKind::Inside,
                            );
                            painter.text(
                                pos2(rect.min.x + 22.0, rect.min.y + 24.0),
                                Align2::CENTER_CENTER,
                                if disk.removable {
                                    icons::PACKAGE
                                } else {
                                    icons::HARD_DRIVE
                                },
                                theme.icon(18.0),
                                theme.accent,
                            );
                            let label = widgets::fit_text(ui, &disk.label(), theme.medium(13.0), card.x - 60.0);
                            let painter = ui.painter();
                            painter.text(
                                pos2(rect.min.x + 40.0, rect.min.y + 24.0),
                                Align2::LEFT_CENTER,
                                label,
                                theme.medium(13.0),
                                theme.fg,
                            );
                            let fraction = disk.used() as f32 / disk.total.max(1) as f32;
                            let bar = Rect::from_min_size(
                                pos2(rect.min.x + 14.0, rect.min.y + 44.0),
                                vec2(card.x - 28.0, 6.0),
                            );
                            let color = if fraction > 0.9 {
                                theme.danger
                            } else if fraction > 0.75 {
                                theme.attention
                            } else {
                                theme.accent
                            };
                            widgets::bar(ui, &theme, bar, fraction, color);
                            ui.painter().text(
                                pos2(rect.min.x + 14.0, rect.min.y + 63.0),
                                Align2::LEFT_CENTER,
                                format!("{} free of {}", human_size(disk.free), human_size(disk.total)),
                                theme.ui(11.5),
                                theme.dim,
                            );
                            ui.painter().text(
                                pos2(rect.max.x - 14.0, rect.min.y + 63.0),
                                Align2::RIGHT_CENTER,
                                &disk.fs,
                                theme.mono(10.5),
                                theme.faint,
                            );
                            if response.clicked() {
                                chosen = Some(disk.mount.clone());
                            }
                        }
                    });
                });
                ui.add_space(gap);
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let width = 346.0;
                ui.add_space((ui.available_width() - width) / 2.0);
                if widgets::primary_button(ui, &theme, &format!("{}  Choose a folder", icons::FOLDER_OPEN)).clicked() {
                    app.choose_folder();
                }
                if let Some(home) = dirstat_core::paths::home_dir()
                    && ui.button(format!("{}  Home folder", icons::HOUSE)).clicked()
                {
                    chosen = Some(home);
                }
                if ui.button(format!("{}  Open saved scan", icons::FILE_TEXT)).clicked() {
                    app.import_dialog();
                }
            });
            if !app.settings.recent.is_empty() {
                ui.add_space(26.0);
                ui.label(RichText::new("Recent").font(theme.medium(12.0)).color(theme.dim));
                ui.add_space(4.0);
                for path in app.settings.recent.clone() {
                    let text = widgets::truncate_middle(&path.display().to_string(), 70);
                    let response = ui.add(
                        egui::Button::new(
                            RichText::new(format!("{}  {text}", icons::CLOCK))
                                .size(12.5)
                                .color(theme.fg),
                        )
                        .frame(false)
                        .min_size(vec2(0.0, 24.0)),
                    );
                    if response.clicked() {
                        chosen = Some(path);
                    }
                }
            }
            ui.add_space(26.0);
            widgets::key_hints(
                ui,
                &theme,
                &[
                    ("Ctrl+O", "Choose a folder"),
                    ("Ctrl+K", "Commands"),
                    ("Drop", "Drop a folder on the window"),
                ],
                280.0,
            );
            ui.add_space(20.0);
        });
    });
    if let Some(path) = chosen {
        app.scan(path);
    }
}
