use egui::{Align2, Color32, Rect, Response, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};

use crate::theme::Theme;

pub fn icon_button(ui: &mut Ui, theme: &Theme, icon: &str, hint: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(icon).font(theme.icon(15.0)).color(theme.dim))
            .frame(false)
            .min_size(vec2(26.0, 26.0)),
    )
    .on_hover_text(hint)
}

pub fn toggle_icon(ui: &mut Ui, theme: &Theme, icon: &str, active: bool, hint: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 26.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, 6, theme.select);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 6, theme.hover);
    }
    let color = if active {
        theme.accent
    } else if response.hovered() {
        theme.fg
    } else {
        theme.dim
    };
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, theme.icon(15.0), color);
    response.on_hover_text(hint)
}

pub fn primary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).font(theme.medium(13.0)).color(theme.accent_fg))
            .fill(theme.accent)
            .stroke(Stroke::NONE)
            .min_size(vec2(72.0, 28.0)),
    )
}

pub fn danger_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).font(theme.medium(13.0)).color(Color32::WHITE))
            .fill(theme.danger)
            .stroke(Stroke::NONE)
            .min_size(vec2(72.0, 28.0)),
    )
}

pub fn keycap(ui: &mut Ui, theme: &Theme, keys: &str) {
    egui::Frame::new()
        .fill(theme.control)
        .stroke(Stroke::new(1.0, theme.line))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(keys).font(theme.mono(10.5)).color(theme.dim)).selectable(false));
        });
}

pub fn section_tabs<T: PartialEq + Copy>(ui: &mut Ui, theme: &Theme, current: &mut T, items: &[(T, String)]) -> bool {
    let mut changed = false;
    let mut underline = None;
    let left = ui.max_rect().left();
    let right = ui.max_rect().right();
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        for (value, label) in items {
            let active = *current == *value;
            let font = if active { theme.medium(12.5) } else { theme.ui(12.5) };
            let galley = ui
                .painter()
                .layout_no_wrap(label.to_string(), font, Color32::PLACEHOLDER);
            let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x, 30.0), Sense::click());
            let color = if active || response.hovered() {
                theme.fg
            } else {
                theme.dim
            };
            ui.painter()
                .galley(pos2(rect.min.x, rect.center().y - galley.size().y / 2.0), galley, color);
            if active {
                underline = Some(rect.x_range());
            }
            if response.clicked() && !active {
                *current = *value;
                changed = true;
            }
        }
    });
    let y = row.response.rect.max.y;
    ui.painter().hline(left..=right, y, Stroke::new(1.0, theme.line));
    if let Some(range) = underline {
        ui.painter().hline(range, y - 0.5, Stroke::new(2.0, theme.accent));
    }
    changed
}

pub fn chip<T: PartialEq + Copy>(ui: &mut Ui, theme: &Theme, current: &mut T, value: T, label: &str) -> bool {
    let active = *current == value;
    let text = RichText::new(label)
        .size(12.0)
        .color(if active { theme.fg } else { theme.dim });
    let response = ui.add(
        egui::Button::new(text)
            .fill(if active { theme.hover } else { Color32::TRANSPARENT })
            .stroke(Stroke::new(1.0, if active { theme.line } else { Color32::TRANSPARENT }))
            .corner_radius(6)
            .min_size(vec2(0.0, 24.0)),
    );
    if response.clicked() && !active {
        *current = value;
        return true;
    }
    false
}

pub fn empty_state(ui: &mut Ui, theme: &Theme, icon: &str, title: &str, hint: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.28);
        empty_heading(ui, theme, icon, title);
        if !hint.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(hint).color(theme.faint).size(12.0));
        }
    });
}

pub fn empty_heading(ui: &mut Ui, theme: &Theme, icon: &str, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 28.0, theme.panel);
    ui.painter()
        .circle_stroke(rect.center(), 28.0, Stroke::new(1.0, theme.line));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        theme.icon(24.0),
        theme.faint,
    );
    ui.add_space(10.0);
    ui.label(RichText::new(title).font(theme.medium(14.0)).color(theme.fg));
}

pub fn key_hints(ui: &mut Ui, theme: &Theme, keys: &[(&str, &str)], width: f32) {
    for (combo, label) in keys {
        ui.allocate_ui_with_layout(
            vec2(width, 26.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(width);
                ui.label(RichText::new(*label).size(12.5).color(theme.dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    keycap(ui, theme, combo);
                });
            },
        );
    }
}

pub fn small(theme: &Theme, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(12.0).color(theme.dim)
}

pub fn heading(theme: &Theme, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).font(theme.semibold(13.0)).color(theme.fg)
}

pub fn spinner(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    let time = ui.input(|i| i.time);
    let angle = (time * 5.0) as f32;
    for step in 0..8 {
        let a = angle + step as f32 * std::f32::consts::TAU / 8.0;
        let alpha = (step as f32 + 1.0) / 8.0;
        ui.painter().circle_filled(
            rect.center() + vec2(a.cos(), a.sin()) * 4.5,
            1.2,
            theme.working.gamma_multiply(alpha),
        );
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(70));
}

pub fn bar(ui: &Ui, theme: &Theme, rect: Rect, fraction: f32, color: Color32) {
    let painter = ui.painter();
    painter.rect_filled(rect, 2, theme.control);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let filled = Rect::from_min_max(rect.min, pos2(rect.min.x + rect.width() * fraction, rect.max.y));
        painter.rect_filled(filled, 2, color);
    }
}

pub fn status_item(ui: &mut Ui, theme: &Theme, icon: &str, label: &str, color: Color32) -> Response {
    let text = if icon.is_empty() {
        label.to_string()
    } else if label.is_empty() {
        icon.to_string()
    } else {
        format!("{icon}  {label}")
    };
    let galley = ui.painter().layout_no_wrap(text, theme.ui(11.5), Color32::PLACEHOLDER);
    let size = galley.size() + vec2(14.0, 0.0);
    let (rect, response) = ui.allocate_exact_size(vec2(size.x, 20.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, theme.hover);
    }
    let shown = if response.hovered() { theme.fg } else { color };
    ui.painter().galley(
        pos2(rect.min.x + 7.0, rect.center().y - galley.size().y / 2.0),
        galley,
        shown,
    );
    response
}

pub fn truncate_middle(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars < 5 {
        return text.to_string();
    }
    let keep = max_chars - 3;
    let head = keep / 3;
    let tail = keep - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}...{end}")
}

pub fn fit_text(ui: &Ui, text: &str, font: egui::FontId, width: f32) -> String {
    let full = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER);
    if full.size().x <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let average = full.size().x / chars.len().max(1) as f32;
    let mut keep = ((width / average) as usize).saturating_sub(1).min(chars.len());
    loop {
        let candidate: String = chars[..keep].iter().collect::<String>() + "...";
        let galley = ui
            .painter()
            .layout_no_wrap(candidate.clone(), font.clone(), Color32::PLACEHOLDER);
        if galley.size().x <= width || keep == 0 {
            return candidate;
        }
        keep -= 1;
    }
}

pub fn fit_middle(ui: &Ui, text: &str, font: egui::FontId, width: f32) -> String {
    let full = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER);
    if full.size().x <= width {
        return text.to_string();
    }
    let count = text.chars().count();
    let average = full.size().x / count.max(1) as f32;
    let mut keep = ((width / average) as usize).min(count);
    loop {
        let candidate = truncate_middle(text, keep);
        let galley = ui
            .painter()
            .layout_no_wrap(candidate.clone(), font.clone(), Color32::PLACEHOLDER);
        if galley.size().x <= width || keep <= 5 {
            return candidate;
        }
        keep -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_paths_keep_both_ends() {
        assert_eq!(truncate_middle("short", 10), "short");
        let cut = truncate_middle("/home/user/projects/very/long/path/file.txt", 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.starts_with("/home"));
        assert!(cut.ends_with("file.txt"));
    }
}
