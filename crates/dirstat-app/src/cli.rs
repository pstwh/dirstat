use std::io::{BufWriter, Write};
use std::path::PathBuf;

use dirstat_core::format::{human_count, human_duration, human_size};
use dirstat_core::scan::{ScanOptions, scan_blocking};
use dirstat_core::{Metric, NodeId, Tree};

pub const HELP: &str = "dirstat                          open the app
dirstat PATH                     open the app and scan PATH
dirstat --open FILE.json         open a saved scan (ncdu JSON)
dirstat scan PATH [options]      scan in the terminal and print the biggest items
  --top N                        items per folder (default 10)
  --depth D                      folder levels to print (default 1)
  --csv FILE                     also export every item as CSV
  --json FILE                    also export as ncdu JSON
  --apparent                     use apparent sizes instead of space on disk
  -x, --cross                    cross into other file systems
  --threads N                    scanner threads (default auto)
  --exclude PATTERN              skip names or absolute paths (repeatable)
DIRSTAT_DATA_DIR                 use another settings folder";

pub struct ScanArgs {
    pub root: PathBuf,
    pub top: usize,
    pub depth: usize,
    pub csv: Option<PathBuf>,
    pub json: Option<PathBuf>,
    pub options: ScanOptions,
}

pub fn parse(args: &[String]) -> Result<ScanArgs, String> {
    let mut root: Option<PathBuf> = None;
    let mut top = 10;
    let mut depth = 1;
    let mut csv = None;
    let mut json = None;
    let mut cross = false;
    let mut apparent = false;
    let mut threads = 0;
    let mut excludes = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--top" => top = value("--top")?.parse().map_err(|_| "--top needs a number")?,
            "--depth" => depth = value("--depth")?.parse().map_err(|_| "--depth needs a number")?,
            "--csv" => csv = Some(PathBuf::from(value("--csv")?)),
            "--json" => json = Some(PathBuf::from(value("--json")?)),
            "--threads" => threads = value("--threads")?.parse().map_err(|_| "--threads needs a number")?,
            "--exclude" => excludes.push(value("--exclude")?),
            "--apparent" => apparent = true,
            "-x" | "--cross" => cross = true,
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => root = Some(PathBuf::from(other)),
        }
    }
    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let root = std::fs::canonicalize(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let root = crate::app::clean_path(root);
    let mut options = ScanOptions::new(&root);
    options.cross_filesystems = cross;
    options.threads = threads;
    options.metric = if apparent { Metric::Apparent } else { Metric::Allocated };
    options.excludes = dirstat_core::exclude::Excludes::parse(&excludes);
    Ok(ScanArgs {
        root,
        top,
        depth,
        csv,
        json,
        options,
    })
}

fn print_level(
    out: &mut impl Write,
    tree: &Tree,
    id: NodeId,
    metric: Metric,
    args: &ScanArgs,
    level: usize,
) -> std::io::Result<()> {
    let total = tree.weight(id, metric).max(1);
    let mut scratch = Vec::new();
    let children: Vec<NodeId> = tree
        .sorted_children(id, metric, &mut scratch)
        .iter()
        .copied()
        .filter(|&c| !tree.node(c).is_dead())
        .collect();
    let indent = "  ".repeat(level);
    for &child in children.iter().take(args.top) {
        let node = tree.node(child);
        let weight = node.weight(metric);
        let share = weight as f64 / total as f64;
        let filled = (share * 10.0).round() as usize;
        let bar: String = "#".repeat(filled) + &".".repeat(10 - filled.min(10));
        let suffix = if node.is_dir() { "/" } else { "" };
        writeln!(
            out,
            "{indent}{:>10}  {:>5.1}%  [{bar}]  {}{suffix}",
            human_size(weight),
            share * 100.0,
            tree.name_lossy(child)
        )?;
        if node.is_dir() && level + 1 < args.depth {
            print_level(out, tree, child, metric, args, level + 1)?;
        }
    }
    if children.len() > args.top {
        let rest: u64 = children[args.top..].iter().map(|&c| tree.weight(c, metric)).sum();
        writeln!(
            out,
            "{indent}{:>10}  {:>5.1}%  and {} more",
            human_size(rest),
            rest as f64 / total as f64 * 100.0,
            human_count((children.len() - args.top) as u64)
        )?;
    }
    Ok(())
}

pub fn run(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("dirstat scan: {error}");
            return 2;
        }
    };
    let metric = parsed.options.metric;
    let (tree, progress) = match scan_blocking(parsed.options.clone()) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("dirstat scan: {error}");
            return 1;
        }
    };
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let root = tree.node(0);
    let _ = writeln!(
        out,
        "{}  {}  {} files  {} folders  {}",
        parsed.root.display(),
        human_size(root.weight(metric)),
        human_count(root.files as u64),
        human_count(root.folders() as u64),
        human_duration(progress.elapsed())
    );
    let _ = print_level(&mut out, &tree, 0, metric, &parsed, 0);
    let errors = tree.errors.len();
    if errors > 0 {
        let _ = writeln!(out, "{} folders could not be read", human_count(errors as u64));
    }
    let _ = out.flush();
    if let Some(path) = &parsed.csv {
        let written = std::fs::File::create(path).and_then(|file| {
            let mut writer = BufWriter::new(file);
            dirstat_core::export::write_csv(&tree, 0, metric, &mut writer)?;
            writer.flush()
        });
        if let Err(error) = written {
            eprintln!("dirstat scan: {}: {error}", path.display());
            return 1;
        }
    }
    if let Some(path) = &parsed.json {
        let written = std::fs::File::create(path).and_then(|file| {
            let mut writer = BufWriter::new(file);
            dirstat_core::export::write_ncdu(&tree, 0, &mut writer)?;
            writer.flush()
        });
        if let Err(error) = written {
            eprintln!("dirstat scan: {}: {error}", path.display());
            return 1;
        }
    }
    0
}

#[cfg(windows)]
pub fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}
