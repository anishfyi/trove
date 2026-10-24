//! Protocol machinery for the read-only trove MCP server. This crate knows
//! JSON-RPC 2.0 framing and MCP method shapes (see SPEC.md); it knows
//! nothing about troves. All behavior is supplied through [`Handler`].

use std::io::{BufRead, Write};

use serde_json::{json, Value};

pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Protocol versions this server can speak. The implemented surface is
/// compatible with both, so the client's version is echoed when supported.
pub const SUPPORTED_VERSIONS: [&str; 2] = ["2024-11-05", "2025-03-26"];

fn negotiate(client: Option<&str>) -> &'static str {
    match client {
        Some(v) if SUPPORTED_VERSIONS.contains(&v) => SUPPORTED_VERSIONS
            .iter()
            .find(|s| **s == v)
            .copied()
            .unwrap_or(PROTOCOL_VERSION),
        _ => PROTOCOL_VERSION,
    }
}

/// One advertised tool.
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// The text result of one tool invocation. An `Err` from the handler is a
/// tool result with `isError: true`, not a protocol error.
pub struct ToolOutput {
    pub text: String,
}

/// One advertised resource.
pub struct ResourceSpec {
    pub uri: String,
    pub name: String,
    pub description: String,
}

/// The body of one resource.
pub struct ResourceContents {
    pub uri: String,
    pub text: String,
}

/// One advertised prompt.
pub struct PromptSpec {
    pub name: String,
    pub description: String,
}

/// The body of one prompt.
pub struct PromptResult {
    pub text: String,
}

/// Everything the protocol needs from the serving application.
pub trait Handler {
    fn server_info(&self) -> (String, String);
    fn tools(&self) -> Vec<ToolSpec>;
    fn call_tool(&self, name: &str, args: &Value) -> Result<ToolOutput, String>;
    fn resources(&self) -> Vec<ResourceSpec>;
    fn read_resource(&self, uri: &str) -> Result<ResourceContents, String>;
    fn prompts(&self) -> Vec<PromptSpec>;
    fn get_prompt(&self, name: &str) -> Result<PromptResult, String>;
}

/// Run the server loop: newline-delimited JSON-RPC 2.0 from `reader`,
/// responses to `writer`, one per request line, until EOF. Notifications
/// (frames without an id) never produce output. Only protocol frames are
/// ever written to `writer`.
pub fn run(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    handler: &dyn Handler,
) -> std::io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(()); // EOF: client went away
        }
        if line.trim().is_empty() {
            continue;
        }
        let frame: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => {
                respond(
                    writer,
                    json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {"code": -32700, "message": "Parse error"}
                    }),
                )?;
                continue;
            }
        };
        let id = frame.get("id").cloned();
        let Some(id) = id else {
            continue; // notification: never respond, never fail
        };
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = frame.get("params").cloned().unwrap_or(json!({}));
        respond(writer, dispatch(handler, id, method, &params))?;
    }
}

fn dispatch(handler: &dyn Handler, id: Value, method: &str, params: &Value) -> Value {
    let result: Result<Value, String> = match method {
        "initialize" => {
            let (name, version) = handler.server_info();
            let client_version = params.get("protocolVersion").and_then(Value::as_str);
            Ok(json!({
                "protocolVersion": negotiate(client_version),
                "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                "serverInfo": {"name": name, "version": version}
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": handler.tools().iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })).collect::<Vec<_>>()
        })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let known = handler.tools().iter().any(|t| t.name == name);
            if !known {
                return error(id, -32602, &format!("unknown tool: {name}"));
            }
            match handler.call_tool(name, &args) {
                Ok(out) => Ok(json!({
                    "content": [{"type": "text", "text": out.text}],
                    "isError": false
                })),
                Err(msg) => Ok(json!({
                    "content": [{"type": "text", "text": msg}],
                    "isError": true
                })),
            }
        }
        "resources/list" => Ok(json!({
            "resources": handler.resources().iter().map(|r| json!({
                "uri": r.uri, "name": r.name, "description": r.description
            })).collect::<Vec<_>>()
        })),
        "resources/read" => {
            let uri = params
                .get("uri")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match handler.read_resource(uri) {
                Ok(c) => Ok(json!({
                    "contents": [{"uri": c.uri, "mimeType": "text/markdown", "text": c.text}]
                })),
                Err(_) => return error(id, -32602, &format!("unknown resource: {uri}")),
            }
        }
        "prompts/list" => Ok(json!({
            "prompts": handler.prompts().iter().map(|p| json!({
                "name": p.name, "description": p.description
            })).collect::<Vec<_>>()
        })),
        "prompts/get" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let known = handler.prompts().iter().any(|p| p.name == name);
            if !known {
                return error(id, -32602, &format!("unknown prompt: {name}"));
            }
            match handler.get_prompt(name) {
                Ok(p) => Ok(json!({
                    "messages": [{
                        "role": "user",
                        "content": {"type": "text", "text": p.text}
                    }]
                })),
                Err(msg) => return error(id, -32603, &msg),
            }
        }
        _ => return error(id, -32601, &format!("method not found: {method}")),
    };
    match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => error(id, -32603, &e.to_string()),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
}

fn respond(writer: &mut dyn Write, frame: Value) -> std::io::Result<()> {
    writeln!(writer, "{frame}")
}
