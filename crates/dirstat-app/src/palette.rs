use std::path::PathBuf;

use dirstat_core::NodeId;
use dirstat_core::format::human_size;
use dirstat_core::search::{Query, search};
use egui::{Align2, Context, Frame, Key, Margin, Modifiers, RichText, Sense, Stroke, vec2};

use crate::app::{Action, App, Dialog, Section};
use crate::chrome;
use crate::icons;
use crate::settings::Appearance;

pub struct Palette {
    pub query: String,
    pub selected: usize,
}

impl Palette {
    pub fn new() -> Palette {
        Palette {
            query: String::new(),
            selected: 0,
        }
    }
}

#[derive(Clone, Debug)]
enum Target {
    Command(Command),
    Scan(PathBuf),
    Node(NodeId),
}

#[derive(Clone, Copy, Debug)]
enum Command {
    Open,
    Import,
    Rescan,
    RescanSelection,
    Stop,
    Search,
    Duplicates,
    Largest,
    Types,
    Folders,
    ZoomIn,
    ZoomOut,
    ZoomRoot,
    Sidebar,
    Treemap,
    Arrangement,
    Metric,
    ExportCsv,
    ExportJson,
    Settings,
    Theme,
    Errors,
    Keys,
    Reveal,
    CopyPath,
    Trash,
    ClearHighlight,
    Close,
}

struct Row {
    target: Target,
    label: String,
    hint: String,
    icon: &'static str,
    keys: &'static str,
}

pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let haystack: Vec<char> = text.to_lowercase().chars().collect();
    let mut total = 0;
    let mut position = 0usize;
    let mut previous: Option<usize> = None;
    for needle in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let found = haystack.get(position..)?.iter().position(|&c| c == needle)? + position;
        total += 10;
        if previous.is_some_and(|p| p + 1 == found) {
            total += 15;
        }
        if found == 0
            || haystack
                .get(found.wrapping_sub(1))
                .is_some_and(|c| !c.is_alphanumeric())
        {
            total += 20;
        }
        total -= (found - position).min(20) as i32;
        previous = Some(found);
        position = found + 1;
    }
    Some(total - haystack.len() as i32 / 8)
}

fn command(command: Command, label: &str, icon: &'static str, keys: &'static str) -> Row {
    Row {
        target: Target::Command(command),
        label: label.into(),
        hint: String::new(),
        icon,
        keys,
    }
}

fn rows(app: &App) -> Vec<Row> {
    let has_tree = app.has_tree();
    let mut rows = vec![
        command(Command::Open, "Scan a folder", icons::FOLDER_OPEN, "Ctrl+O"),
        command(Command::Import, "Open a saved scan (ncdu JSON)", icons::FILE_TEXT, ""),
    ];
    if has_tree {
        if app.is_scanning() {
            rows.push(command(Command::Stop, "Stop the scan", icons::STOP, "Esc"));
        }
        rows.extend([
            command(Command::Rescan, "Rescan everything", icons::REFRESH, "F5"),
            command(
                Command::Close,
                "Close the scan and pick another drive",
                icons::HOUSE,
                "Ctrl+W",
            ),
            command(
                Command::RescanSelection,
                "Rescan the selected folder",
                icons::REFRESH,
                "Shift+F5",
            ),
            command(Command::Search, "Search files", icons::SEARCH, "Ctrl+F"),
            command(
                Command::Duplicates,
                "Find duplicate files",
                icons::FINGERPRINT,
                "Ctrl+D",
            ),
            command(Command::Largest, "Largest files", icons::FILES, "Alt+3"),
            command(Command::Types, "File types", icons::CHART_PIE, "Alt+2"),
            command(Command::Folders, "Folders", icons::TREE, "Alt+1"),
            command(Command::ZoomIn, "Zoom into the selection", icons::ZOOM_IN, "Enter"),
            command(Command::ZoomOut, "Zoom out", icons::ARROW_ELBOW_LEFT_UP, "Backspace"),
            command(Command::ZoomRoot, "Zoom to the top", icons::HARD_DRIVE, "Home"),
            command(Command::Sidebar, "Show or hide the lists", icons::SIDEBAR, "Ctrl+B"),
            command(Command::Treemap, "Show or hide the treemap", icons::TREEMAP, "Ctrl+M"),
            command(
                Command::Arrangement,
                "Side by side or stacked",
                icons::COLUMNS,
                "Ctrl+\\",
            ),
            command(
                Command::Metric,
                "Switch between on disk and apparent size",
                icons::GAUGE,
                "",
            ),
            command(Command::ExportCsv, "Export as CSV", icons::EXPORT, "Ctrl+E"),
            command(Command::ExportJson, "Export as ncdu JSON", icons::EXPORT, ""),
            command(Command::Errors, "Show unreadable folders", icons::WARNING, ""),
            command(Command::ClearHighlight, "Clear the type highlight", icons::CLOSE, ""),
        ]);
        if app.selected.is_some() {
            rows.extend([
                command(
                    Command::Reveal,
                    "Show the selection in the file manager",
                    icons::OPEN_OUT,
                    "Ctrl+Shift+R",
                ),
                command(
                    Command::CopyPath,
                    "Copy the path of the selection",
                    icons::COPY,
                    "Ctrl+Shift+C",
                ),
                command(Command::Trash, "Move the selection to the trash", icons::TRASH, "Del"),
            ]);
        }
    }
    rows.extend([
        command(Command::Settings, "Settings", icons::GEAR, "Ctrl+,"),
        command(Command::Theme, "Change the theme", icons::MOON, ""),
        command(Command::Keys, "Keyboard shortcuts", icons::KEYBOARD, "F1"),
    ]);
    for disk in &app.disks {
        rows.push(Row {
            target: Target::Scan(disk.mount.clone()),
            label: format!("Scan {}", disk.label()),
            hint: format!("{} free of {}", human_size(disk.free), human_size(disk.total)),
            icon: icons::HARD_DRIVE,
            keys: "",
        });
    }
    for path in &app.settings.recent {
        rows.push(Row {
            target: Target::Scan(path.clone()),
            label: format!("Scan {}", path.display()),
            hint: "recent".into(),
            icon: icons::CLOCK,
            keys: "",
        });
    }
    rows
}

fn node_matches(app: &App, query: &str) -> Vec<NodeId> {
    let Some(tree) = app.shared() else { return Vec::new() };
    if query.trim().chars().count() < 2 || app.is_scanning() {
        return Vec::new();
    }
    let tree = tree.read();
    search(&tree, &Query::parse(query), 0, app.settings.metric, 30)
}

pub fn show(app: &mut App, ctx: &Context) {
    let Some(mut palette) = app.palette.take() else { return };
    let theme = app.theme.clone();
    let mut ranked: Vec<(i32, Row)> = rows(app)
        .into_iter()
        .filter_map(|row| score(&palette.query, &row.label).map(|s| (s, row)))
        .collect();
    if !palette.query.is_empty() {
        ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        ranked.truncate(12);
    }
    let cached = ctx.memory(|m| m.data.get_temp::<String>(egui::Id::unique("palette-nodes-query")));
    let nodes = if cached.as_deref() == Some(palette.query.as_str()) {
        ctx.memory(|m| m.data.get_temp::<Vec<NodeId>>(egui::Id::unique("palette-nodes")))
            .unwrap_or_default()
    } else {
        let found = node_matches(app, &palette.query);
        ctx.memory_mut(|m| {
            m.data
                .insert_temp(egui::Id::unique("palette-nodes-query"), palette.query.clone());
            m.data.insert_temp(egui::Id::unique("palette-nodes"), found.clone());
        });
        found
    };
    if !nodes.is_empty()
        && let Some(tree) = app.shared()
    {
        let tree = tree.read();
        for id in nodes {
            let Some(node) = tree.get(id) else { continue };
            ranked.push((
                0,
                Row {
                    target: Target::Node(id),
                    label: tree.display_name(id),
                    hint: format!(
                        "{}  {}",
                        human_size(node.weight(app.settings.metric)),
                        tree.path(node.parent.min(tree.len() as u32 - 1)).display()
                    ),
                    icon: if node.is_dir() { icons::FOLDER } else { icons::FILE },
                    keys: "",
                },
            ));
        }
    }
    let mut chosen: Option<Target> = None;
    let mut close = false;
    ctx.input_mut(|input| {
        if input.consume_key(Modifiers::NONE, Key::Escape) {
            close = true;
        }
        if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
            palette.selected = (palette.selected + 1).min(ranked.len().saturating_sub(1));
        }
        if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
            palette.selected = palette.selected.saturating_sub(1);
        }
        if input.consume_key(Modifiers::NONE, Key::Enter)
            && let Some((_, row)) = ranked.get(palette.selected)
        {
            chosen = Some(row.target.clone());
        }
    });
    let screen = ctx.content_rect();
    let width = 640.0f32.min(screen.width() - 40.0);
    let area = egui::Area::new(egui::Id::unique("dirstat-palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(screen.center().x - width / 2.0, screen.min.y + 80.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(theme.raised)
                .stroke(Stroke::new(1.0, theme.line))
                .corner_radius(8)
                .shadow(ui.visuals().popup_shadow)
                .inner_margin(Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(width);
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut palette.query)
                            .hint_text("Type a command, or a file name to jump to it")
                            .desired_width(f32::INFINITY)
                            .font(theme.ui(14.0)),
                    );
                    response.request_focus();
                    if response.changed() {
                        palette.selected = 0;
                    }
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (index, (_, row)) in ranked.iter().enumerate() {
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                            let painter = ui.painter();
                            if index == palette.selected || response.hovered() {
                                painter.rect_filled(rect, 4, theme.select);
                            }
                            if index == palette.selected {
                                response.scroll_to_me(None);
                            }
                            painter.text(
                                egui::pos2(rect.min.x + 16.0, rect.center().y),
                                Align2::CENTER_CENTER,
                                row.icon,
                                theme.icon(14.0),
                                theme.dim,
                            );
                            let x = rect.min.x + 36.0;
                            let label = painter.layout_no_wrap(row.label.clone(), theme.ui(13.0), theme.fg);
                            let label_width = label.size().x;
                            painter.galley(egui::pos2(x, rect.center().y - label.size().y / 2.0), label, theme.fg);
                            if !row.hint.is_empty() {
                                let room = rect.max.x - x - label_width - 110.0;
                                let max_chars = (room / 6.3).max(0.0) as usize;
                                let hint = crate::widgets::truncate_middle(&row.hint, max_chars);
                                painter.text(
                                    egui::pos2(x + label_width + 12.0, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    hint,
                                    theme.ui(11.5),
                                    theme.faint,
                                );
                            }
                            if !row.keys.is_empty() {
                                painter.text(
                                    egui::pos2(rect.max.x - 10.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    row.keys,
                                    theme.mono(10.5),
                                    theme.dim,
                                );
                            }
                            if response.clicked() {
                                chosen = Some(row.target.clone());
                            }
                        }
                        if ranked.is_empty() {
                            ui.label(RichText::new("Nothing matches").color(theme.faint));
                        }
                    });
                });
        });
    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos))
    });
    if clicked_outside {
        close = true;
    }
    if let Some(target) = chosen {
        run(app, target);
        return;
    }
    if !close {
        app.palette = Some(palette);
    }
}

fn run(app: &mut App, target: Target) {
    match target {
        Target::Scan(path) => app.scan(path),
        Target::Node(id) => {
            app.actions.push(Action::Select(id));
            app.section = Section::Folders;
            app.settings.show_sidebar = true;
        }
        Target::Command(command) => run_command(app, command),
    }
}

fn run_command(app: &mut App, command: Command) {
    let selection = app.selection_or_zoom();
    match command {
        Command::Open => app.choose_folder(),
        Command::Import => app.import_dialog(),
        Command::Rescan => app.rescan_all(),
        Command::RescanSelection => app.actions.push(Action::Rescan(selection)),
        Command::Stop => app.stop(),
        Command::Search => {
            app.section = Section::Search;
            app.settings.show_sidebar = true;
            app.search.focus = true;
        }
        Command::Duplicates => {
            app.section = Section::Duplicates;
            app.settings.show_sidebar = true;
        }
        Command::Largest => show_section(app, Section::Largest),
        Command::Types => show_section(app, Section::Types),
        Command::Folders => show_section(app, Section::Folders),
        Command::ZoomIn => app.actions.push(Action::Zoom(selection)),
        Command::ZoomOut => app.actions.push(Action::ZoomOut),
        Command::ZoomRoot => app.actions.push(Action::ZoomRoot),
        Command::Sidebar => {
            app.settings.show_sidebar = !app.settings.show_sidebar;
            app.save_settings_soon();
        }
        Command::Treemap => {
            app.settings.show_treemap = !app.settings.show_treemap;
            app.save_settings_soon();
        }
        Command::Arrangement => chrome::toggle_arrangement(app),
        Command::Metric => app.set_metric(app.settings.metric.other()),
        Command::ExportCsv => app.export(false),
        Command::ExportJson => app.export(true),
        Command::Settings => chrome::open_settings(app),
        Command::Theme => {
            app.settings.appearance = match app.settings.appearance {
                Appearance::System => Appearance::Dark,
                Appearance::Dark => Appearance::Light,
                Appearance::Light => Appearance::System,
            };
            app.save_settings_soon();
        }
        Command::Errors => app.dialog = Some(Dialog::Errors),
        Command::Keys => app.dialog = Some(Dialog::Keys),
        Command::Reveal => app.actions.push(Action::Reveal(selection)),
        Command::CopyPath => app.actions.push(Action::CopyPaths(app.selection())),
        Command::Trash => app.actions.push(Action::Trash(app.selection())),
        Command::Close => app.close(),
        Command::ClearHighlight => app.highlight_ext = None,
    }
}

pub fn show_section(app: &mut App, section: Section) {
    app.section = section;
    app.settings.show_sidebar = true;
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn prefix_and_word_starts_rank_higher() {
        let exact = score("res", "Rescan everything").unwrap();
        let inner = score("res", "Show unreadable folders, rest").unwrap();
        assert!(exact > inner);
        assert!(score("xyz", "Rescan").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }
}
