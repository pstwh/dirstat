use std::path::{Path, PathBuf};

use crate::tree::{NodeFlags, NodeId, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    Always,
    Sibling(&'static str),
    Child(&'static str),
    UnderHome(&'static str),
}

#[derive(Debug)]
pub struct Rule {
    pub kind: &'static str,
    pub project: bool,
    pub names: &'static [&'static str],
    pub need: Need,
    pub why: &'static str,
}

pub const RULES: &[Rule] = &[
    Rule {
        kind: "node_modules",
        project: true,
        names: &["node_modules"],
        need: Need::Sibling("package.json"),
        why: "JavaScript packages of a project, restored by npm, yarn or pnpm install in the folder above",
    },
    Rule {
        kind: "rust_target",
        project: true,
        names: &["target"],
        need: Need::Sibling("Cargo.toml"),
        why: "Rust build output, rebuilt by cargo build",
    },
    Rule {
        kind: "python_cache",
        project: true,
        names: &["__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".tox"],
        need: Need::Always,
        why: "Python caches, recreated when the code runs or the tools run again",
    },
    Rule {
        kind: "python_venv",
        project: true,
        names: &[".venv", "venv", "env"],
        need: Need::Child("pyvenv.cfg"),
        why: "Python virtual environment, recreated with python -m venv and a reinstall of the requirements",
    },
    Rule {
        kind: "web_build_cache",
        project: true,
        names: &[".next", ".nuxt", ".svelte-kit", ".parcel-cache", ".turbo", ".angular"],
        need: Need::Always,
        why: "Web framework build cache, rebuilt on the next build",
    },
    Rule {
        kind: "gradle",
        project: true,
        names: &[".gradle"],
        need: Need::Always,
        why: "Gradle caches, downloaded again on the next build",
    },
    Rule {
        kind: "xcode_derived_data",
        project: false,
        names: &["DerivedData"],
        need: Need::Always,
        why: "Xcode build data, rebuilt on the next build",
    },
    Rule {
        kind: "terraform",
        project: true,
        names: &[".terraform"],
        need: Need::Always,
        why: "Terraform providers and modules, restored by terraform init",
    },
    Rule {
        kind: "user_cache",
        project: false,
        names: &[".cache"],
        need: Need::UnderHome(""),
        why: "Per-user application caches, programs rebuild what they need",
    },
    Rule {
        kind: "package_cache",
        project: false,
        names: &["registry", "_cacache", "caches"],
        need: Need::UnderHome(".cargo|.npm|.gradle"),
        why: "Package manager download cache, fetched again when needed",
    },
    Rule {
        kind: "macos_caches",
        project: false,
        names: &["Caches"],
        need: Need::UnderHome("Library"),
        why: "macOS application caches, programs rebuild what they need",
    },
    Rule {
        kind: "windows_temp",
        project: false,
        names: &["Temp"],
        need: Need::UnderHome("AppData/Local"),
        why: "Windows temporary files, files in use cannot be removed and are skipped",
    },
    Rule {
        kind: "trash",
        project: false,
        names: &["Trash", ".Trash"],
        need: Need::UnderHome(".local/share|"),
        why: "Files already in the trash, gone for good once the trash is emptied",
    },
];

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub node: NodeId,
    pub rule: usize,
}

fn has_child(tree: &Tree, parent: NodeId, name: &str) -> bool {
    tree.live_children(parent)
        .any(|child| tree.name_bytes(child) == name.as_bytes())
}

fn matches(tree: &Tree, node: NodeId, rule: &Rule, home: Option<&Path>) -> bool {
    let name = tree.name_bytes(node);
    if !rule.names.iter().any(|n| n.as_bytes() == name) {
        return false;
    }
    let parent = tree.node(node).parent;
    match rule.need {
        Need::Always => true,
        Need::Sibling(file) => parent != u32::MAX && has_child(tree, parent, file),
        Need::Child(file) => has_child(tree, node, file),
        Need::UnderHome(places) => {
            let Some(home) = home else { return false };
            if parent == u32::MAX {
                return false;
            }
            let parent_path = tree.path(parent);
            places.split('|').any(|place| {
                parent_path
                    == if place.is_empty() {
                        home.to_path_buf()
                    } else {
                        home.join(place)
                    }
            })
        }
    }
}

pub fn system_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        let mut roots: Vec<PathBuf> = ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"]
            .iter()
            .filter_map(std::env::var_os)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .collect();
        if roots.is_empty() {
            roots = [
                "C:\\Windows",
                "C:\\Program Files",
                "C:\\Program Files (x86)",
                "C:\\ProgramData",
            ]
            .iter()
            .map(PathBuf::from)
            .collect();
        }
        return roots;
    }
    let list: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System",
            "/Library",
            "/Applications",
            "/usr",
            "/opt",
            "/bin",
            "/sbin",
            "/private",
        ]
    } else {
        &[
            "/usr", "/opt", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/etc", "/boot", "/var/lib", "/snap", "/nix",
        ]
    };
    list.iter().map(PathBuf::from).collect()
}

pub fn is_system(path: &Path) -> bool {
    system_roots().iter().any(|root| path.starts_with(root))
}

const APP_DATA: [&str; 3] = ["snap", "Library", "AppData"];

pub fn find(tree: &Tree, within: NodeId, home: Option<&Path>) -> Vec<Hit> {
    if is_system(&tree.path(within)) {
        return Vec::new();
    }
    let skip: Vec<NodeId> = system_roots().iter().filter_map(|root| tree.find_path(root)).collect();
    let home_node = home.and_then(|home| tree.find_path(home));
    let within_app_data = home.is_some_and(|home| {
        tree.path(within)
            .strip_prefix(home)
            .ok()
            .and_then(|rest| rest.components().next())
            .is_some_and(|first| {
                let name = first.as_os_str().to_string_lossy();
                name.starts_with('.') || APP_DATA.contains(&name.as_ref())
            })
    });
    let mut hits = Vec::new();
    let mut stack = vec![(within, within_app_data)];
    while let Some((node, app_data)) = stack.pop() {
        for child in tree.live_children(node) {
            let entry = tree.node(child);
            if !entry.is_dir() || entry.flags.has(NodeFlags::SYMLINK) || skip.contains(&child) {
                continue;
            }
            let name = tree.name_bytes(child);
            let rule = RULES
                .iter()
                .position(|rule| (!app_data || !rule.project) && matches(tree, child, rule, home));
            match rule {
                Some(rule) => hits.push(Hit { node: child, rule }),
                None if name == b"node_modules" => {}
                None => {
                    let enters_app_data = Some(node) == home_node
                        && (name.starts_with(b".") || APP_DATA.iter().any(|dir| dir.as_bytes() == name));
                    stack.push((child, app_data || enters_app_data));
                }
            }
        }
    }
    hits
}
