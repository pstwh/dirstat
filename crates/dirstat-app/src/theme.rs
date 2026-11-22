use egui::{Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke, Visuals};

const fn hex(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub base: Color32,
    pub panel: Color32,
    pub field: Color32,
    pub control: Color32,
    pub raised: Color32,
    pub hover: Color32,
    pub line: Color32,
    pub fg: Color32,
    pub dim: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_fg: Color32,
    pub working: Color32,
    pub attention: Color32,
    pub ok: Color32,
    pub danger: Color32,
    pub select: Color32,
    pub font_size: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

pub const MONO: &str = "mono";
pub const ICONS: &str = "icons";
pub const ICONS_FILL: &str = "icons-fill";
pub const MEDIUM: &str = "medium";
pub const SEMIBOLD: &str = "semibold";

impl Theme {
    pub fn dark() -> Theme {
        Theme {
            dark: true,
            base: hex(0x16171A),
            panel: hex(0x1C1D21),
            field: hex(0x111215),
            control: hex(0x26272C),
            raised: hex(0x222328),
            hover: hex(0x2E2F35),
            line: hex(0x2C2D33),
            fg: hex(0xE4E4E8),
            dim: hex(0x9C9DA7),
            faint: hex(0x666771),
            accent: hex(0x5FD3BC),
            accent_fg: hex(0x0B1C18),
            working: hex(0x6CA8F0),
            attention: hex(0xE5B567),
            ok: hex(0x8CC265),
            danger: hex(0xE8666F),
            select: hex(0x1D3833),
            font_size: 13.0,
        }
    }

    pub fn light() -> Theme {
        Theme {
            dark: false,
            base: hex(0xFFFFFF),
            panel: hex(0xF5F5F7),
            field: hex(0xFFFFFF),
            control: hex(0xF1F1F4),
            raised: hex(0xFFFFFF),
            hover: hex(0xE9E9ED),
            line: hex(0xDFDFE5),
            fg: hex(0x1C1D21),
            dim: hex(0x5C5E67),
            faint: hex(0x9A9CA5),
            accent: hex(0x0B7D6B),
            accent_fg: hex(0xFFFFFF),
            working: hex(0x2B6BD3),
            attention: hex(0xA96A00),
            ok: hex(0x2C8240),
            danger: hex(0xCF3242),
            select: hex(0xDCF1EB),
            font_size: 13.0,
        }
    }

    pub fn new(dark: bool, font_size: f32) -> Theme {
        let mut theme = if dark { Theme::dark() } else { Theme::light() };
        theme.font_size = font_size;
        theme
    }

    pub fn mono(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(MONO.into()))
    }

    pub fn ui(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Proportional)
    }

    pub fn medium(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(MEDIUM.into()))
    }

    pub fn semibold(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
    }

    pub fn icon(&self, size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(ICONS.into()))
    }

    pub fn rgb(&self, color: [u8; 3]) -> Color32 {
        Color32::from_rgb(color[0], color[1], color[2])
    }

    pub fn base_rgb(&self) -> [u8; 3] {
        [self.base.r(), self.base.g(), self.base.b()]
    }

    pub fn install_fonts(ctx: &Context) {
        let mut fonts = FontDefinitions::default();
        let faces: [(&str, &'static [u8]); 6] = [
            ("JetBrainsMono", include_bytes!("../../../assets/JetBrainsMono.ttf")),
            ("Inter", include_bytes!("../../../assets/Inter-Regular.ttf")),
            ("Inter-Medium", include_bytes!("../../../assets/Inter-Medium.ttf")),
            ("Inter-SemiBold", include_bytes!("../../../assets/Inter-SemiBold.ttf")),
            ("Phosphor", include_bytes!("../../../assets/Phosphor.ttf")),
            ("Phosphor-Fill", include_bytes!("../../../assets/Phosphor-Fill.ttf")),
        ];
        for (name, bytes) in faces {
            fonts
                .font_data
                .insert(name.to_string(), FontData::from_static(bytes).into());
        }
        let fallback = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
        let stack = |primary: &[&str]| {
            let mut list: Vec<String> = primary.iter().map(|s| s.to_string()).collect();
            list.push("Phosphor".into());
            list.extend(fallback.iter().cloned());
            list
        };
        fonts
            .families
            .insert(FontFamily::Proportional, stack(&["Inter", "JetBrainsMono"]));
        fonts.families.insert(FontFamily::Monospace, stack(&["JetBrainsMono"]));
        fonts
            .families
            .insert(FontFamily::Name(MONO.into()), stack(&["JetBrainsMono"]));
        fonts
            .families
            .insert(FontFamily::Name(MEDIUM.into()), stack(&["Inter-Medium"]));
        fonts
            .families
            .insert(FontFamily::Name(SEMIBOLD.into()), stack(&["Inter-SemiBold"]));
        fonts
            .families
            .insert(FontFamily::Name(ICONS.into()), vec!["Phosphor".into(), "Inter".into()]);
        fonts.families.insert(
            FontFamily::Name(ICONS_FILL.into()),
            vec!["Phosphor-Fill".into(), "Phosphor".into()],
        );
        ctx.set_fonts(fonts);
    }

    pub fn install(&self, ctx: &Context) {
        let mut visuals = if self.dark { Visuals::dark() } else { Visuals::light() };
        let shadow_alpha = if self.dark { 120 } else { 34 };
        visuals.panel_fill = self.panel;
        visuals.window_fill = self.raised;
        visuals.extreme_bg_color = self.field;
        visuals.text_edit_bg_color = Some(self.field);
        visuals.faint_bg_color = if self.dark { hex(0x1B1C20) } else { hex(0xF7F7F9) };
        visuals.code_bg_color = self.field;
        visuals.window_stroke = Stroke::new(1.0, self.line);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.window_shadow = Shadow {
            offset: [0, 10],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        };
        visuals.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        };
        visuals.selection.bg_fill = if self.dark {
            self.accent.gamma_multiply(0.32)
        } else {
            self.accent.gamma_multiply(0.22)
        };
        visuals.selection.stroke = Stroke::new(1.0, self.accent);
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.attention;
        visuals.error_fg_color = self.danger;
        visuals.override_text_color = Some(self.fg);
        visuals.weak_text_color = Some(self.faint);
        visuals.text_cursor.stroke = Stroke::new(1.5, self.accent);
        visuals.collapsing_header_frame = false;
        visuals.striped = false;
        visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        for (widget, fill) in [
            (&mut visuals.widgets.noninteractive, self.panel),
            (&mut visuals.widgets.inactive, self.control),
            (&mut visuals.widgets.hovered, self.hover),
            (&mut visuals.widgets.active, self.hover),
            (&mut visuals.widgets.open, self.hover),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.corner_radius = CornerRadius::same(6);
            widget.bg_stroke = Stroke::new(1.0, self.line);
            widget.fg_stroke = Stroke::new(1.0, self.fg);
            widget.expansion = 0.0;
        }
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.dim);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.fg);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.faint);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, self.faint);
        ctx.set_theme(if self.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });
        let mono_size = self.font_size;
        ctx.all_styles_mut(|style| {
            style.visuals = visuals.clone();
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            style.spacing.button_padding = egui::vec2(10.0, 4.0);
            style.spacing.interact_size.y = 26.0;
            style.spacing.icon_width = 15.0;
            style.spacing.icon_width_inner = 9.0;
            style.spacing.menu_margin = egui::Margin::same(6);
            style.spacing.window_margin = egui::Margin::same(18);
            style.spacing.combo_height = 320.0;
            style.spacing.scroll = egui::style::ScrollStyle::floating();
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, FontId::proportional(11.5));
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                FontId::new(mono_size, FontFamily::Name(MONO.into())),
            );
            style.text_styles.insert(
                egui::TextStyle::Heading,
                FontId::new(17.0, FontFamily::Name(SEMIBOLD.into())),
            );
            style.interaction.selectable_labels = true;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f32 {
        let channel = |value: u8| {
            let v = value as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_is_readable_in_both_themes() {
        for theme in [Theme::dark(), Theme::light()] {
            assert!(contrast(theme.fg, theme.base) >= 7.0, "fg on base, dark={}", theme.dark);
            assert!(
                contrast(theme.dim, theme.base) >= 4.5,
                "dim on base, dark={}",
                theme.dark
            );
            assert!(
                contrast(theme.dim, theme.panel) >= 4.5,
                "dim on panel, dark={}",
                theme.dark
            );
            assert!(
                contrast(theme.accent_fg, theme.accent) >= 4.5,
                "accent button, dark={}",
                theme.dark
            );
        }
    }
}
