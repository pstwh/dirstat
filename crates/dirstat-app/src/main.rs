#![cfg_attr(windows, windows_subsystem = "windows")]

mod actions;
mod app;
mod chrome;
mod cli;
mod icons;
mod keys;
mod lists_view;
mod palette;
mod settings;
mod start_view;
mod theme;
mod tree_view;
mod treemap_view;
mod widgets;

use std::path::PathBuf;

use egui::{Frame, Margin, Stroke};

use app::{App, Section};
use settings::Arrangement;
use theme::Theme;

enum Launch {
    Scan(PathBuf),
    Import(PathBuf),
}

struct Shell {
    app: App,
    launch: Option<Launch>,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(launch) = self.launch.take() {
            match launch {
                Launch::Scan(path) => self.app.scan(path),
                Launch::Import(path) => self.app.import(path),
            }
        }
        self.app.sync_theme(&ctx);
        self.app.last_hovered = self.app.hovered.take();
        dropped_files(&mut self.app, &ctx);
        self.app.tick(&ctx);
        keys::handle(&mut self.app, &ctx);
        let theme = self.app.theme.clone();
        egui::Panel::bottom("dirstat-status")
            .exact_size(26.0)
            .frame(
                Frame::new()
                    .fill(theme.panel)
                    .stroke(Stroke::new(1.0, theme.line))
                    .inner_margin(Margin::symmetric(8, 0)),
            )
            .show(ui, |ui| chrome::status_bar(&mut self.app, ui));
        egui::Panel::top("dirstat-top")
            .exact_size(chrome::TOP_HEIGHT)
            .frame(Frame::new().fill(theme.panel))
            .show(ui, |ui| chrome::top_bar(&mut self.app, ui));
        if self.app.has_tree() {
            workspace(&mut self.app, ui);
        } else {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(theme.base))
                .show(ui, |ui| start_view::show(&mut self.app, ui));
        }
        palette::show(&mut self.app, &ctx);
        chrome::dialogs(&mut self.app, &ctx);
        chrome::toasts(&mut self.app, &ctx);
        self.app.apply_actions(&ctx);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(job) = &self.app.job {
            job.cancel();
        }
        self.app.cancel_duplicates();
        self.app.settings.save();
    }
}

fn dropped_files(app: &mut App, ctx: &egui::Context) {
    let dropped: Vec<PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    });
    let Some(path) = dropped.into_iter().next() else { return };
    if path.is_dir() {
        app.scan(path);
    } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
        app.import(path);
    } else if let Some(parent) = path.parent() {
        app.scan(parent.to_path_buf());
    }
}

fn lists(app: &mut App, ui: &mut egui::Ui) {
    let theme = app.theme.clone();
    egui::Frame::new()
        .inner_margin(Margin {
            left: 12,
            right: 12,
            top: 2,
            bottom: 0,
        })
        .show(ui, |ui| {
            let mut section = app.section;
            let items: Vec<(Section, String)> = [
                Section::Folders,
                Section::Types,
                Section::Largest,
                Section::Duplicates,
                Section::Search,
            ]
            .into_iter()
            .map(|s| {
                let label = chrome::section_label(s).to_string();
                let label = if s == Section::Duplicates && app.dupes.progress.is_some() {
                    format!("{label} ...")
                } else {
                    label
                };
                (s, label)
            })
            .collect();
            if widgets::section_tabs(ui, &theme, &mut section, &items) {
                app.section = section;
                if section == Section::Search {
                    app.search.focus = true;
                }
            }
        });
    match app.section {
        Section::Folders => tree_view::show(app, ui),
        Section::Types => lists_view::types(app, ui),
        Section::Largest => lists_view::largest(app, ui),
        Section::Duplicates => lists_view::duplicates(app, ui),
        Section::Search => lists_view::search_view(app, ui),
    }
}

fn workspace(app: &mut App, ui: &mut egui::Ui) {
    let theme = app.theme.clone();
    let show_lists = app.settings.show_sidebar || !app.settings.show_treemap;
    let show_treemap = app.settings.show_treemap;
    let list_frame = Frame::new().fill(theme.base);
    if show_lists && show_treemap {
        let available = ui.available_rect_before_wrap();
        match app.settings.arrangement {
            Arrangement::SideBySide => {
                egui::Panel::left("dirstat-lists")
                    .default_size((available.width() * 0.46).clamp(360.0, 900.0))
                    .size_range(300.0..=(available.width() - 200.0).max(320.0))
                    .resizable(true)
                    .frame(list_frame.stroke(Stroke::new(1.0, theme.line)))
                    .show(ui, |ui| lists(app, ui));
            }
            Arrangement::Stacked => {
                egui::Panel::top("dirstat-lists-top")
                    .default_size((available.height() * 0.45).max(160.0))
                    .size_range(120.0..=(available.height() - 120.0).max(140.0))
                    .resizable(true)
                    .frame(list_frame.stroke(Stroke::new(1.0, theme.line)))
                    .show(ui, |ui| lists(app, ui));
            }
        }
    }
    egui::CentralPanel::default()
        .frame(Frame::new().fill(theme.base))
        .show(ui, |ui| {
            if show_treemap {
                treemap_view::show(app, ui);
            } else {
                lists(app, ui);
            }
        });
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let first = args.get(1).map(String::as_str);
    if matches!(first, Some("--help" | "-h")) {
        cli::attach_console();
        println!("{}", cli::HELP);
        return Ok(());
    }
    if matches!(first, Some("--version" | "-V")) {
        cli::attach_console();
        println!("dirstat {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if first == Some("mcp") {
        std::process::exit(cli::mcp(&args[2..]));
    }
    if first == Some("scan") {
        cli::attach_console();
        std::process::exit(cli::run(&args[2..]));
    }
    let launch = match first {
        Some("--open") => args.get(2).map(|p| Launch::Import(PathBuf::from(p))),
        Some(path) => Some(Launch::Scan(PathBuf::from(path))),
        None => None,
    };
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()
        .map(|image| {
            let rgba = image.to_rgba8();
            let (width, height) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("dirstat")
        .with_app_id("dirstat")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([760.0, 460.0])
        .with_drag_and_drop(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "dirstat",
        options,
        Box::new(|creation| {
            let ctx = creation.egui_ctx.clone();
            Theme::install_fonts(&ctx);
            let mut app = App::new(&ctx);
            app.sync_theme(&ctx);
            app.theme.install(&ctx);
            Ok(Box::new(Shell { app, launch }))
        }),
    )
}
