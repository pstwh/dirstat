use dirstat_core::{NodeId, Tree};
use egui::{Align, Layout, RichText, Ui};

use crate::app::{Action, App};
use crate::icons;

fn item(ui: &mut Ui, app: &App, icon: &str, label: &str, keys: &str) -> bool {
    let theme = &app.theme;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(270.0, 26.0), egui::Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 4, theme.hover);
    }
    let color = if response.hovered() { theme.fg } else { theme.dim };
    painter.text(
        egui::pos2(rect.min.x + 14.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        icon,
        theme.icon(14.0),
        color,
    );
    painter.text(
        egui::pos2(rect.min.x + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme.ui(12.5),
        theme.fg,
    );
    if !keys.is_empty() {
        painter.text(
            egui::pos2(rect.max.x - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            keys,
            theme.mono(10.5),
            theme.faint,
        );
    }
    response.clicked()
}

pub fn context_menu(app: &mut App, tree: &Tree, ui: &mut Ui, node: NodeId) {
    let (is_dir, is_root, name) = {
        let Some(entry) = tree.get(node) else {
            ui.close();
            return;
        };
        (entry.is_dir(), node == tree.root, tree.display_name(node))
    };
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.label(
            RichText::new(crate::widgets::truncate_middle(&name, 40))
                .font(app.theme.medium(12.0))
                .color(app.theme.dim),
        );
        ui.separator();
        let mut chosen = None;
        if item(ui, app, icons::OPEN_OUT, "Open", "Ctrl+Enter") {
            chosen = Some(Action::Open(node));
        }
        if item(ui, app, icons::FOLDER_OPEN, "Show in file manager", "Ctrl+Shift+R") {
            chosen = Some(Action::Reveal(node));
        }
        if item(ui, app, icons::COPY, "Copy path", "Ctrl+Shift+C") {
            chosen = Some(Action::CopyPath(node));
        }
        if item(ui, app, icons::CLIPBOARD, "Copy name", "") {
            chosen = Some(Action::CopyName(node));
        }
        ui.separator();
        if is_dir && item(ui, app, icons::ZOOM_IN, "Zoom into this folder", "Enter") {
            chosen = Some(Action::Zoom(node));
        }
        if app.zoom != 0 && item(ui, app, icons::ARROW_ELBOW_LEFT_UP, "Zoom out", "Backspace") {
            chosen = Some(Action::ZoomOut);
        }
        if item(ui, app, icons::REFRESH, "Rescan this folder", "Shift+F5") {
            chosen = Some(Action::Rescan(node));
        }
        if !is_root && item(ui, app, icons::EYE_SLASH, "Exclude this name from scans", "") {
            chosen = Some(Action::Exclude(node));
        }
        if !is_root {
            ui.separator();
            if item(ui, app, icons::TRASH, "Move to trash", "Del") {
                chosen = Some(Action::Trash(vec![node]));
            }
            if item(ui, app, icons::WARNING, "Delete permanently", "Shift+Del") {
                chosen = Some(Action::Delete(vec![node]));
            }
        }
        if let Some(action) = chosen {
            app.actions.push(action);
            ui.close();
        }
    });
}
