use std::time::Instant;

use dirstat_core::format::{human_count, human_size};
use dirstat_core::scan::{ScanOptions, scan_blocking};

fn peak_rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|l| l.starts_with("VmHWM"))
                .map(|l| l.trim_start_matches("VmHWM:").trim().to_string())
        })
        .unwrap_or_else(|| "n/a".into())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let mut options = ScanOptions::new(&root);
    if let Some(threads) = args.get(2).and_then(|t| t.parse().ok()) {
        options.threads = threads;
    }
    let threads = options.thread_count();
    let started = Instant::now();
    let (tree, progress) = match scan_blocking(options) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let elapsed = started.elapsed();
    let entries = tree.len() as f64;
    println!("root      {root}");
    println!("threads   {threads}");
    println!("entries   {}", human_count(tree.len() as u64));
    println!("size      {}", human_size(tree.node(0).size));
    println!("allocated {}", human_size(tree.node(0).alloc));
    println!(
        "errors    {}",
        progress.errors.load(std::sync::atomic::Ordering::Relaxed)
    );
    println!("time      {:.3} s", elapsed.as_secs_f64());
    println!("rate      {}/s", human_count((entries / elapsed.as_secs_f64()) as u64));
    println!(
        "memory    {:.1} B/entry (nodes+order+names)",
        (tree.nodes.capacity() * 48 + tree.order.capacity() * 4 + tree.names.capacity()) as f64 / entries
    );
    println!("peak rss  {}", peak_rss());
}
