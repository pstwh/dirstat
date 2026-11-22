use std::path::Path;

use dirstat_core::paths::clean_path;
use dirstat_mcp::{Options, Server};
use serde_json::{Value, json};

fn write(root: &Path, relative: &str, bytes: usize, fill: u8) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![fill; bytes]).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let root = dir.path();
    write(root, "web/package.json", 100, b'a');
    write(root, "web/node_modules/left-pad/index.js", 40_000, b'b');
    write(root, "web/node_modules/react/index.js", 60_000, b'c');
    write(root, "rust/Cargo.toml", 100, b'd');
    write(root, "rust/target/debug/app", 200_000, b'e');
    write(root, "photos/target/keep.jpg", 30_000, b'f');
    write(root, "py/__pycache__/mod.pyc", 10_000, b'g');
    write(root, "global/lib/node_modules/npm/index.js", 90_000, b'j');
    write(root, "global/lib/node_modules/npm/package.json", 100, b'k');
    write(
        root,
        "global/lib/node_modules/npm/node_modules/dep/index.js",
        70_000,
        b'l',
    );
    write(root, "media/movie.mp4", 500_000, b'h');
    write(root, "media/copy/movie.mp4", 500_000, b'h');
    write(root, "docs/report.pdf", 20_000, b'i');
    dir
}

fn call(server: &mut Server, name: &str, arguments: Value) -> (Value, bool) {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments },
    });
    let reply = server.handle(&request).unwrap();
    let result = &reply["result"];
    let text = result["content"][0]["text"].as_str().unwrap();
    (
        serde_json::from_str(text).unwrap(),
        result["isError"].as_bool().unwrap(),
    )
}

#[test]
fn initialize_and_list_read_only_tools() {
    let mut server = Server::new(Options::default());
    let reply = server
        .handle(&json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }))
        .unwrap();
    assert_eq!(reply["result"]["serverInfo"]["name"], "dirstat");
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    let reply = server
        .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .unwrap();
    let tools = reply["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "disks",
            "scan",
            "top",
            "largest_files",
            "types",
            "search",
            "duplicates",
            "cleanup_candidates"
        ]
    );
    assert!(tools.iter().all(|t| t["annotations"]["readOnlyHint"] == true));
    assert!(tools.iter().all(|t| t["annotations"]["destructiveHint"] == false));
}

#[test]
fn scan_top_largest_types_and_search() {
    let dir = fixture();
    let root = dir.path().to_str().unwrap();
    let mut server = Server::new(Options::default());
    let (scan, error) = call(&mut server, "scan", json!({ "path": root }));
    assert!(!error, "{scan}");
    assert_eq!(scan["files"], "13");
    assert_eq!(scan["top"][0]["name"], "media");
    let (top, _) = call(
        &mut server,
        "top",
        json!({ "path": format!("{root}/media"), "depth": 2, "metric": "apparent" }),
    );
    assert_eq!(top["children"][0]["kind"], "dir");
    assert_eq!(top["children"][0]["children"][0]["name"], "movie.mp4");
    assert_eq!(top["children"][0]["children"][0]["size"], "488 KB");
    let (largest, _) = call(&mut server, "largest_files", json!({ "path": root, "limit": 2 }));
    let files = largest["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files[0]["path"].as_str().unwrap().ends_with("movie.mp4"));
    let (types, _) = call(&mut server, "types", json!({ "path": root, "metric": "apparent" }));
    assert_eq!(types["types"][0]["extension"], ".mp4");
    assert_eq!(types["types"][0]["files"], 2);
    let (found, _) = call(&mut server, "search", json!({ "path": root, "query": "ext:pdf" }));
    assert_eq!(found["shown"], 1);
    assert!(found["results"][0]["path"].as_str().unwrap().ends_with("report.pdf"));
}

#[test]
fn tools_scan_on_their_own_and_reuse_covering_scans() {
    let dir = fixture();
    let root = dir.path().to_str().unwrap();
    let mut server = Server::new(Options::default());
    let (top, error) = call(&mut server, "top", json!({ "path": format!("{root}/rust") }));
    assert!(!error, "{top}");
    assert!(top["scan"]["root"].as_str().unwrap().ends_with("rust"));
    call(&mut server, "scan", json!({ "path": root }));
    let (top, _) = call(&mut server, "top", json!({ "path": format!("{root}/web") }));
    let canonical = clean_path(std::fs::canonicalize(root).unwrap());
    assert_eq!(top["scan"]["root"].as_str().unwrap(), canonical.to_str().unwrap());
}

#[test]
fn cleanup_finds_regenerable_folders_only() {
    let dir = fixture();
    let root = dir.path().to_str().unwrap();
    let mut server = Server::new(Options::default());
    let (cleanup, error) = call(&mut server, "cleanup_candidates", json!({ "path": root }));
    assert!(!error, "{cleanup}");
    let kinds: Vec<&str> = cleanup["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["rust_target", "node_modules", "python_cache"]);
    let text = cleanup.to_string();
    assert!(!text.contains("photos"));
    assert!(!text.contains("global"));
    assert!(!text.contains("left-pad"));
}

#[test]
fn duplicates_group_identical_files() {
    let dir = fixture();
    let root = dir.path().to_str().unwrap();
    let mut server = Server::new(Options::default());
    let (dupes, error) = call(&mut server, "duplicates", json!({ "path": root, "min_size": "300kb" }));
    assert!(!error, "{dupes}");
    assert_eq!(dupes["groups"], 1);
    assert_eq!(dupes["results"][0]["copies"], 2);
    let (none, _) = call(&mut server, "duplicates", json!({ "path": format!("{root}/docs") }));
    assert_eq!(none["groups"], 0);
}

#[test]
fn bad_input_is_reported_as_a_tool_error() {
    let mut server = Server::new(Options::default());
    let (relative, error) = call(&mut server, "top", json!({ "path": "relative/path" }));
    assert!(error);
    assert!(relative["error"].as_str().unwrap().contains("absolute"));
    let (missing, error) = call(&mut server, "top", json!({ "path": "/no/such/dirstat/folder" }));
    assert!(error);
    assert!(missing["error"].as_str().unwrap().contains("/no/such"));
    let reply = server
        .handle(&json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": "delete", "arguments": {} } }))
        .unwrap();
    assert_eq!(reply["error"]["code"], -32602);
}
