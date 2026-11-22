use egui::{Context, Key, Modifiers};

use crate::app::{Action, App, Dialog, Section};
use crate::chrome;
use crate::palette::{Palette, show_section};

fn pressed(ctx: &Context, modifiers: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| i.consume_key(modifiers, key))
}

const COMMAND_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};

pub fn handle(app: &mut App, ctx: &Context) {
    if app.dialog.is_some() {
        return;
    }
    if pressed(ctx, Modifiers::COMMAND, Key::K) || pressed(ctx, COMMAND_SHIFT, Key::P) {
        app.palette = if app.palette.is_some() {
            None
        } else {
            Some(Palette::new())
        };
    }
    if app.palette.is_some() {
        return;
    }
    if pressed(ctx, Modifiers::COMMAND, Key::O) {
        app.choose_folder();
    }
    if pressed(ctx, Modifiers::COMMAND, Key::Comma) {
        chrome::open_settings(app);
    }
    if pressed(ctx, Modifiers::NONE, Key::F1) {
        app.dialog = Some(Dialog::Keys);
    }
    if !app.has_tree() {
        return;
    }
    if pressed(ctx, Modifiers::COMMAND, Key::F) {
        show_section(app, Section::Search);
        app.search.focus = true;
    }
    if pressed(ctx, Modifiers::COMMAND, Key::B) {
        app.settings.show_sidebar = !app.settings.show_sidebar;
        app.save_settings_soon();
    }
    if pressed(ctx, Modifiers::COMMAND, Key::M) {
        app.settings.show_treemap = !app.settings.show_treemap;
        app.save_settings_soon();
    }
    if pressed(ctx, Modifiers::COMMAND, Key::Backslash) {
        chrome::toggle_arrangement(app);
    }
    if pressed(ctx, Modifiers::COMMAND, Key::E) {
        app.export(false);
    }
    if pressed(ctx, Modifiers::COMMAND, Key::D) {
        show_section(app, Section::Duplicates);
        if app.dupes.progress.is_none() {
            app.find_duplicates();
        }
    }
    if pressed(ctx, Modifiers::SHIFT, Key::F5) {
        app.actions.push(Action::Rescan(app.selection_or_zoom()));
    }
    if pressed(ctx, Modifiers::NONE, Key::F5) || pressed(ctx, Modifiers::COMMAND, Key::R) {
        app.rescan_all();
    }
    let sections = [
        (Key::Num1, Section::Folders),
        (Key::Num2, Section::Types),
        (Key::Num3, Section::Largest),
        (Key::Num4, Section::Duplicates),
        (Key::Num5, Section::Search),
    ];
    for (key, section) in sections {
        if pressed(ctx, Modifiers::ALT, key) {
            show_section(app, section);
            if section == Section::Search {
                app.search.focus = true;
            }
        }
    }
    let selection = app.selected;
    if let Some(node) = selection {
        if pressed(ctx, COMMAND_SHIFT, Key::R) {
            app.actions.push(Action::Reveal(node));
        }
        if pressed(ctx, COMMAND_SHIFT, Key::C) {
            app.actions.push(Action::CopyPath(node));
        }
        if pressed(ctx, Modifiers::COMMAND, Key::Enter) {
            app.actions.push(Action::Open(node));
        }
    }
    let typing = ctx.memory(|m| m.focused().is_some());
    if typing {
        if pressed(ctx, Modifiers::NONE, Key::Escape) {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        return;
    }
    if pressed(ctx, Modifiers::NONE, Key::Escape) {
        if app.highlight_ext.is_some() {
            app.highlight_ext = None;
        } else if app.is_scanning() {
            app.stop();
        } else {
            app.selected = None;
        }
    }
    if pressed(ctx, Modifiers::NONE, Key::Backspace) || pressed(ctx, Modifiers::ALT, Key::ArrowUp) {
        app.actions.push(Action::ZoomOut);
    }
    if let Some(node) = selection {
        if pressed(ctx, Modifiers::NONE, Key::Enter) {
            app.actions.push(Action::Zoom(node));
        }
        if pressed(ctx, Modifiers::SHIFT, Key::Delete) {
            app.actions.push(Action::Delete(vec![node]));
        }
        if pressed(ctx, Modifiers::NONE, Key::Delete) {
            app.actions.push(Action::Trash(vec![node]));
        }
    }
    navigate(app, ctx);
}

fn navigate(app: &mut App, ctx: &Context) {
    let rows = &app.folders.rows;
    if rows.is_empty() {
        return;
    }
    let current = app.selected.and_then(|s| rows.iter().position(|r| r.node == s));
    let page = 20;
    let mut target: Option<usize> = None;
    if pressed(ctx, Modifiers::NONE, Key::ArrowDown) {
        target = Some(current.map(|c| c + 1).unwrap_or(0));
    }
    if pressed(ctx, Modifiers::NONE, Key::ArrowUp) {
        target = Some(current.map(|c| c.saturating_sub(1)).unwrap_or(0));
    }
    if pressed(ctx, Modifiers::NONE, Key::PageDown) {
        target = Some(current.map(|c| c + page).unwrap_or(0));
    }
    if pressed(ctx, Modifiers::NONE, Key::PageUp) {
        target = Some(current.map(|c| c.saturating_sub(page)).unwrap_or(0));
    }
    if pressed(ctx, Modifiers::NONE, Key::Home) {
        target = Some(0);
    }
    if pressed(ctx, Modifiers::NONE, Key::End) {
        target = Some(rows.len() - 1);
    }
    let right = pressed(ctx, Modifiers::NONE, Key::ArrowRight);
    let left = pressed(ctx, Modifiers::NONE, Key::ArrowLeft);
    if (right || left)
        && let Some(index) = current
        && let Some(tree) = app.shared()
    {
        let row = rows[index];
        let (is_dir, has_children, parent) = {
            let tree = tree.read();
            let node = tree.node(row.node);
            (node.is_dir(), node.child_count > 0, node.parent)
        };
        let open = app.folders.expanded.contains(&row.node);
        if right && is_dir && has_children {
            if open {
                target = Some(index + 1);
            } else {
                app.actions.push(Action::Expand(row.node, true));
            }
        }
        if left {
            if open && is_dir {
                app.actions.push(Action::Expand(row.node, false));
            } else if parent != u32::MAX {
                target = rows.iter().position(|r| r.node == parent);
            }
        }
    }
    if let Some(index) = target {
        let index = index.min(app.folders.rows.len() - 1);
        let node = app.folders.rows[index].node;
        app.selected = Some(node);
        app.folders.scroll_to = Some(node);
        if app.section != Section::Folders {
            app.section = Section::Folders;
        }
    }
}
