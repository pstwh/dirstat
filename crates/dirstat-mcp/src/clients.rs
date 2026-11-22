use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use dirstat_core::paths::write_atomic;
use serde_json::{Map, Value, json};

const SERVER_NAME: &str = "dirstat";
const CODEX_HEADER: &str = "[mcp_servers.dirstat]";
const CODEX_SUBTABLE: &str = "[mcp_servers.dirstat.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    Cursor,
    VsCode,
    GeminiCli,
    Windsurf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientStatus {
    NotFound,
    Available,
    Installed,
}

impl Client {
    pub const ALL: [Client; 7] = [
        Client::ClaudeCode,
        Client::ClaudeDesktop,
        Client::Codex,
        Client::Cursor,
        Client::VsCode,
        Client::GeminiCli,
        Client::Windsurf,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude-code",
            Client::ClaudeDesktop => "claude-desktop",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
            Client::VsCode => "vscode",
            Client::GeminiCli => "gemini",
            Client::Windsurf => "windsurf",
        }
    }

    pub fn parse(name: &str) -> Option<Client> {
        let wanted = name.to_ascii_lowercase().replace(['_', ' '], "-");
        Client::ALL.into_iter().find(|client| client.slug() == wanted)
    }

    pub fn label(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
            Client::VsCode => "VS Code",
            Client::GeminiCli => "Gemini CLI",
            Client::Windsurf => "Windsurf",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Locations {
    pub home: PathBuf,
    pub config: PathBuf,
    pub codex_home: PathBuf,
    pub path: Option<OsString>,
}

impl Locations {
    pub fn system() -> Locations {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let config = if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        } else if cfg!(target_os = "macos") {
            home.join("Library").join("Application Support")
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
        };
        let codex_home = std::env::var_os("CODEX_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        Locations {
            home,
            config,
            codex_home,
            path: std::env::var_os("PATH"),
        }
    }

    pub fn config_file(&self, client: Client) -> PathBuf {
        match client {
            Client::ClaudeCode => self.home.join(".claude.json"),
            Client::ClaudeDesktop => self.config.join("Claude").join("claude_desktop_config.json"),
            Client::Codex => self.codex_home.join("config.toml"),
            Client::Cursor => self.home.join(".cursor").join("mcp.json"),
            Client::VsCode => self.config.join("Code").join("User").join("mcp.json"),
            Client::GeminiCli => self.home.join(".gemini").join("settings.json"),
            Client::Windsurf => self.home.join(".codeium").join("windsurf").join("mcp_config.json"),
        }
    }

    fn program(&self, client: Client) -> Option<PathBuf> {
        let name = match client {
            Client::ClaudeCode => "claude",
            Client::Codex => "codex",
            Client::VsCode => "code",
            Client::GeminiCli => "gemini",
            _ => return None,
        };
        self.find(name)
    }

    fn find(&self, program: &str) -> Option<PathBuf> {
        let path = self.path.as_ref()?;
        let extensions: &[&str] = if cfg!(windows) {
            &[".exe", ".cmd", ".bat", ""]
        } else {
            &[""]
        };
        std::env::split_paths(path).find_map(|dir| {
            extensions
                .iter()
                .map(|extension| dir.join(format!("{program}{extension}")))
                .find(|candidate| candidate.is_file())
        })
    }

    fn app_folder(&self, client: Client) -> PathBuf {
        match client {
            Client::ClaudeCode => self.home.join(".claude"),
            Client::ClaudeDesktop => self.config.join("Claude"),
            Client::Codex => self.codex_home.clone(),
            Client::Cursor => self.home.join(".cursor"),
            Client::VsCode => self.config.join("Code"),
            Client::GeminiCli => self.home.join(".gemini"),
            Client::Windsurf => self.home.join(".codeium").join("windsurf"),
        }
    }
}

fn arguments() -> Vec<String> {
    vec!["mcp".to_string()]
}

fn json_key(client: Client) -> &'static str {
    match client {
        Client::VsCode => "servers",
        _ => "mcpServers",
    }
}

fn json_entry(client: Client, exe: &Path) -> Value {
    let command = exe.display().to_string();
    match client {
        Client::VsCode | Client::ClaudeCode => {
            json!({ "type": "stdio", "command": command, "args": arguments() })
        }
        _ => json!({ "command": command, "args": arguments() }),
    }
}

fn shell_quote(text: &str) -> String {
    let plain = text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/\\._-:+=@".contains(c));
    if plain {
        text.to_string()
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn toml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn codex_block(exe: &Path) -> String {
    let args: Vec<String> = arguments().iter().map(|a| toml_string(a)).collect();
    format!(
        "{CODEX_HEADER}\ncommand = {}\nargs = [{}]\n",
        toml_string(&exe.display().to_string()),
        args.join(", ")
    )
}

fn command_line(exe: &Path) -> String {
    let mut parts = vec![shell_quote(&exe.display().to_string())];
    parts.extend(arguments());
    parts.join(" ")
}

pub fn snippet(client: Client, exe: &Path) -> String {
    match client {
        Client::ClaudeCode => format!("claude mcp add --scope user {SERVER_NAME} -- {}", command_line(exe)),
        Client::Codex => codex_block(exe),
        _ => {
            let mut servers = Map::new();
            servers.insert(SERVER_NAME.into(), json_entry(client, exe));
            let mut root = Map::new();
            root.insert(json_key(client).into(), Value::Object(servers));
            format!(
                "{}\n",
                serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
            )
        }
    }
}

pub fn status(client: Client, exe: &Path) -> ClientStatus {
    status_in(&Locations::system(), client, exe)
}

pub fn install(client: Client, exe: &Path) -> Result<String, String> {
    install_in(&Locations::system(), client, exe)
}

pub fn uninstall(client: Client) -> Result<String, String> {
    uninstall_in(&Locations::system(), client)
}

pub fn status_in(locations: &Locations, client: Client, exe: &Path) -> ClientStatus {
    let file = locations.config_file(client);
    let command = exe.display().to_string();
    let configured = match client {
        Client::Codex => std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| codex_command(&text))
            .map(|found| found == command),
        _ => read_object(&file)
            .ok()
            .flatten()
            .and_then(|root| {
                root.get(json_key(client))
                    .and_then(|servers| servers.get(SERVER_NAME))
                    .cloned()
            })
            .map(|entry| entry.get("command").and_then(Value::as_str) == Some(command.as_str())),
    };
    match configured {
        Some(true) => ClientStatus::Installed,
        Some(false) => ClientStatus::Available,
        None if locations.program(client).is_some() || locations.app_folder(client).exists() => ClientStatus::Available,
        None => ClientStatus::NotFound,
    }
}

pub fn install_in(locations: &Locations, client: Client, exe: &Path) -> Result<String, String> {
    match client {
        Client::ClaudeCode => {
            let Some(claude) = locations.program(client) else {
                return Err(format!(
                    "The claude command was not found. Run this in a terminal: {}",
                    snippet(client, exe)
                ));
            };
            let _ = run(&claude, &["mcp", "remove", "--scope", "user", SERVER_NAME]);
            let mut args: Vec<String> = ["mcp", "add", "--scope", "user", SERVER_NAME, "--"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            args.push(exe.display().to_string());
            args.extend(arguments());
            run(&claude, &args)?;
            Ok("Added dirstat to Claude Code for your user. Restart running Claude Code sessions.".into())
        }
        Client::Codex => match locations.program(client) {
            Some(codex) => {
                let _ = run(&codex, &["mcp", "remove", SERVER_NAME]);
                let mut args: Vec<String> = ["mcp", "add", SERVER_NAME, "--"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                args.push(exe.display().to_string());
                args.extend(arguments());
                run(&codex, &args)?;
                Ok("Added dirstat to Codex. Restart running Codex sessions.".into())
            }
            None => {
                let file = locations.config_file(client);
                edit_codex(&file, Some(&codex_block(exe)))?;
                Ok(format!("Added dirstat to {}", file.display()))
            }
        },
        Client::VsCode => {
            if let Some(code) = locations.program(client) {
                let mut entry = json_entry(client, exe);
                if let Value::Object(map) = &mut entry {
                    map.insert("name".into(), json!(SERVER_NAME));
                }
                run(&code, &["--add-mcp".to_string(), entry.to_string()])?;
                return Ok("Added dirstat to VS Code.".into());
            }
            let file = locations.config_file(client);
            edit_json(&file, json_key(client), Some(json_entry(client, exe)))?;
            Ok(format!("Added dirstat to {}", file.display()))
        }
        _ => {
            let file = locations.config_file(client);
            edit_json(&file, json_key(client), Some(json_entry(client, exe)))?;
            Ok(format!(
                "Added dirstat to {}. Restart {} to load it.",
                file.display(),
                client.label()
            ))
        }
    }
}

pub fn uninstall_in(locations: &Locations, client: Client) -> Result<String, String> {
    match client {
        Client::ClaudeCode if locations.program(client).is_some() => {
            let claude = locations.program(client).unwrap_or_default();
            run(&claude, &["mcp", "remove", "--scope", "user", SERVER_NAME])?;
            Ok("Removed dirstat from Claude Code.".into())
        }
        Client::Codex => {
            let file = locations.config_file(client);
            match locations.program(client) {
                Some(codex) => {
                    run(&codex, &["mcp", "remove", SERVER_NAME])?;
                    Ok("Removed dirstat from Codex.".into())
                }
                None if edit_codex(&file, None)? => Ok(format!("Removed dirstat from {}", file.display())),
                None => Ok("dirstat was not configured in Codex.".into()),
            }
        }
        _ => {
            let file = locations.config_file(client);
            if edit_json(&file, json_key(client), None)? {
                Ok(format!("Removed dirstat from {}", file.display()))
            } else {
                Ok(format!("dirstat was not configured in {}.", client.label()))
            }
        }
    }
}

fn run<S: AsRef<std::ffi::OsStr>>(program: &Path, args: &[S]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not run {}: {error}", program.display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(format!(
            "{} failed: {}",
            program.display(),
            if stderr.is_empty() { stdout } else { stderr }
        ))
    }
}

fn backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut name = path.file_name().map(OsString::from).unwrap_or_default();
    name.push(".bak");
    std::fs::copy(path, path.with_file_name(name))
        .map(|_| ())
        .map_err(|error| format!("could not back up {}: {error}", path.display()))
}

fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    if text.trim().is_empty() {
        return Ok(Some(Map::new()));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(format!("{} is not a JSON object", path.display())),
        Err(error) => Err(format!(
            "{} could not be read as JSON ({error}); add dirstat by hand",
            path.display()
        )),
    }
}

pub(crate) fn edit_json(path: &Path, key: &str, entry: Option<Value>) -> Result<bool, String> {
    let existing = read_object(path)?;
    let mut root = existing.clone().unwrap_or_default();
    match entry {
        Some(entry) => {
            let servers = root.entry(key.to_string()).or_insert_with(|| json!({}));
            if !servers.is_object() {
                return Err(format!("{key} in {} is not an object", path.display()));
            }
            if let Value::Object(map) = servers {
                map.insert(SERVER_NAME.into(), entry);
            }
        }
        None => {
            let removed = root
                .get_mut(key)
                .and_then(Value::as_object_mut)
                .and_then(|servers| servers.remove(SERVER_NAME));
            if removed.is_none() {
                return Ok(false);
            }
        }
    }
    if existing.as_ref() == Some(&root) {
        return Ok(false);
    }
    backup(path)?;
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?
    );
    write_atomic(path, text.as_bytes()).map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(true)
}

fn block_range(text: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    let mut start = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        match start {
            None if trimmed == CODEX_HEADER => start = Some(offset),
            Some(begin) if trimmed.starts_with('[') && !trimmed.starts_with(CODEX_SUBTABLE) => {
                return Some((begin, offset));
            }
            _ => {}
        }
        offset += line.len();
    }
    start.map(|begin| (begin, text.len()))
}

fn codex_command(text: &str) -> Option<String> {
    let (start, end) = block_range(text)?;
    text[start..end].lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim() != "command" {
            return None;
        }
        let value = value.trim();
        let inner = value.strip_prefix('"')?.strip_suffix('"')?;
        Some(inner.replace("\\\\", "\\").replace("\\\"", "\""))
    })
}

pub(crate) fn replace_codex_block(text: &str, block: Option<&str>) -> String {
    match (block_range(text), block) {
        (Some((start, end)), replacement) => {
            let after = &text[end..];
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..start]);
            if let Some(block) = replacement {
                out.push_str(block);
                if !after.is_empty() {
                    out.push('\n');
                }
            }
            out.push_str(after);
            out
        }
        (None, Some(block)) => {
            let mut out = text.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.trim().is_empty() {
                out.push('\n');
            }
            out.push_str(block);
            out
        }
        (None, None) => text.to_string(),
    }
}

fn edit_codex(path: &Path, block: Option<&str>) -> Result<bool, String> {
    let original = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let updated = replace_codex_block(&original, block);
    if updated == original {
        return Ok(false);
    }
    backup(path)?;
    write_atomic(path, updated.as_bytes()).map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locations(root: &Path) -> Locations {
        Locations {
            home: root.join("home"),
            config: root.join("config"),
            codex_home: root.join("home").join(".codex"),
            path: None,
        }
    }

    #[test]
    fn json_merge_keeps_other_servers_and_keys() {
        let dir = tempfile::tempdir().unwrap();
        let places = locations(dir.path());
        let file = places.config_file(Client::Cursor);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            "{\n  \"theme\": \"dark\",\n  \"mcpServers\": {\n    \"other\": { \"command\": \"x\" }\n  }\n}\n",
        )
        .unwrap();
        let exe = Path::new("/opt/dirstat/bin/dirstat");
        assert_eq!(status_in(&places, Client::Cursor, exe), ClientStatus::Available);
        install_in(&places, Client::Cursor, exe).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written["theme"], "dark");
        assert_eq!(written["mcpServers"]["other"]["command"], "x");
        assert_eq!(written["mcpServers"]["dirstat"]["command"], "/opt/dirstat/bin/dirstat");
        assert_eq!(written["mcpServers"]["dirstat"]["args"], json!(["mcp"]));
        assert!(file.with_file_name("mcp.json.bak").exists());
        assert_eq!(status_in(&places, Client::Cursor, exe), ClientStatus::Installed);
        assert_eq!(
            status_in(&places, Client::Cursor, Path::new("/elsewhere/dirstat")),
            ClientStatus::Available
        );
        uninstall_in(&places, Client::Cursor).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert!(written["mcpServers"].get("dirstat").is_none());
        assert_eq!(written["mcpServers"]["other"]["command"], "x");
    }

    #[test]
    fn json_install_creates_missing_files_and_refuses_broken_ones() {
        let dir = tempfile::tempdir().unwrap();
        let places = locations(dir.path());
        let exe = Path::new("/usr/bin/dirstat");
        assert_eq!(status_in(&places, Client::ClaudeDesktop, exe), ClientStatus::NotFound);
        install_in(&places, Client::ClaudeDesktop, exe).unwrap();
        let file = places.config_file(Client::ClaudeDesktop);
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(written["mcpServers"]["dirstat"]["args"], json!(["mcp"]));
        let vscode = places.config_file(Client::VsCode);
        std::fs::create_dir_all(vscode.parent().unwrap()).unwrap();
        std::fs::write(&vscode, "{ // comment\n \"servers\": {} }").unwrap();
        assert!(install_in(&places, Client::VsCode, exe).is_err());
        assert_eq!(
            std::fs::read_to_string(&vscode).unwrap(),
            "{ // comment\n \"servers\": {} }"
        );
    }

    #[test]
    fn codex_block_is_replaced_and_the_rest_kept_byte_for_byte() {
        let original = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other\"\n\n[mcp_servers.dirstat]\ncommand = \"/old/dirstat\"\nargs = [\"mcp\"]\n\n[mcp_servers.dirstat.env]\nA = \"1\"\n\n[profiles.fast]\nmodel = \"x\"\n";
        let block = codex_block(Path::new("C:\\Tools\\dirstat.exe"));
        let updated = replace_codex_block(original, Some(&block));
        assert!(updated.starts_with("model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other\"\n\n"));
        assert!(updated.ends_with("[profiles.fast]\nmodel = \"x\"\n"));
        assert!(updated.contains("command = \"C:\\\\Tools\\\\dirstat.exe\""));
        assert!(!updated.contains("/old/dirstat"));
        assert!(!updated.contains("[mcp_servers.dirstat.env]"));
        assert_eq!(codex_command(&updated).as_deref(), Some("C:\\Tools\\dirstat.exe"));
        let removed = replace_codex_block(&updated, None);
        assert_eq!(
            removed,
            "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"other\"\n\n[profiles.fast]\nmodel = \"x\"\n"
        );
        let appended = replace_codex_block("model = \"o3\"", Some(&block));
        assert!(appended.starts_with("model = \"o3\"\n\n[mcp_servers.dirstat]\n"));
    }

    #[test]
    fn codex_without_the_command_edits_the_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let places = locations(dir.path());
        let file = places.config_file(Client::Codex);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "model = \"o3\"\n").unwrap();
        let exe = Path::new("/usr/local/bin/dirstat");
        install_in(&places, Client::Codex, exe).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("model = \"o3\"\n"));
        assert!(text.contains("args = [\"mcp\"]"));
        assert_eq!(status_in(&places, Client::Codex, exe), ClientStatus::Installed);
        uninstall_in(&places, Client::Codex).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "model = \"o3\"\n\n");
    }

    #[test]
    fn claude_code_without_the_command_reports_what_to_run() {
        let dir = tempfile::tempdir().unwrap();
        let places = locations(dir.path());
        let error = install_in(&places, Client::ClaudeCode, Path::new("/My Apps/dirstat")).unwrap_err();
        assert!(error.contains("claude mcp add --scope user dirstat -- \"/My Apps/dirstat\" mcp"));
    }

    #[test]
    fn snippets_name_the_executable() {
        let exe = Path::new("/usr/bin/dirstat");
        assert!(snippet(Client::VsCode, exe).contains("\"servers\""));
        assert!(snippet(Client::Windsurf, exe).contains("\"mcp\""));
        assert!(snippet(Client::Codex, exe).starts_with(CODEX_HEADER));
    }
}
