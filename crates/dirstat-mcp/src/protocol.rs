use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::tools::{Options, Outcome, Tools};

pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "dirstat measures disk usage fast. Call disks to see drives and free space, or scan to \
measure a folder or drive (a whole drive can take from seconds to a few minutes). The other tools reuse the last scan \
that covers the path and scan on their own when there is none: top for the biggest folders, largest_files, types for \
space by file extension, search with filters like ext:mp4 size:>1gb older:1y, duplicates, and cleanup_candidates for \
caches and build output that can be recreated. dirstat never changes or deletes files. Explain what uses space and \
what is safe to remove, and let the user remove it, for example from the dirstat app. Do not delete files yourself \
unless the user explicitly asks for that specific removal.";

pub struct Server {
    tools: Tools,
}

impl Server {
    pub fn new(options: Options) -> Server {
        Server {
            tools: Tools::new(options),
        }
    }

    pub fn run(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            for reply in self.handle_line(&line) {
                writeln!(output, "{reply}")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    pub fn handle_line(&mut self, line: &str) -> Vec<Value> {
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(messages)) => messages.iter().filter_map(|m| self.handle(m)).collect(),
            Ok(message) => self.handle(&message).into_iter().collect(),
            Err(error) => vec![failure(Value::Null, PARSE_ERROR, &format!("Parse error: {error}"))],
        }
    }

    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            if message.get("result").is_some() || message.get("error").is_some() {
                return None;
            }
            return Some(failure(id.unwrap_or(Value::Null), INVALID_REQUEST, "Invalid request"));
        };
        let id = id?;
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tools.definitions() })),
            "tools/call" => self.call(&params),
            other => Err((METHOD_NOT_FOUND, format!("Method not found: {other}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, text)) => failure(id, code, &text),
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "tools/call needs a tool name".to_string()))?;
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(value) if value.is_object() => value.clone(),
            Some(_) => return Err((INVALID_PARAMS, "arguments must be an object".to_string())),
        };
        match self.tools.call(name, &arguments) {
            Outcome::Unknown => Err((INVALID_PARAMS, format!("Unknown tool: {name}"))),
            Outcome::Done(Ok(value)) => Ok(tool_result(&value, false)),
            Outcome::Done(Err(error)) => Ok(tool_result(&json!({ "error": error }), true)),
        }
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
    let version = if PROTOCOL_VERSIONS.contains(&requested) {
        requested
    } else {
        LATEST_PROTOCOL_VERSION
    };
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "dirstat", "title": "dirstat", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tool_result(value: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string(value).unwrap_or_default();
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
