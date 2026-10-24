//! Protocol-level transcripts with a stub handler: framing, lifecycle,
//! error shapes. The trove behavior itself is exercised by the trove-cli
//! transcript tests against the real binary; these tests pin the
//! protocol machinery only.

use std::io::Cursor;

use trove_mcp::{
    Handler, PromptResult, PromptSpec, ResourceContents, ResourceSpec, ToolOutput, ToolSpec,
};

struct Stub;

impl Handler for Stub {
    fn server_info(&self) -> (String, String) {
        ("trove-stub".into(), "9.9.9".into())
    }

    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "boom".into(),
                description: "Always fails".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {}, "required": []}),
            },
            ToolSpec {
                name: "echo".into(),
                description: "Echo the question".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"q": {"type": "string"}},
                    "required": ["q"]
                }),
            },
        ]
    }

    fn call_tool(&self, name: &str, args: &serde_json::Value) -> Result<ToolOutput, String> {
        match name {
            "echo" => Ok(ToolOutput {
                text: args["q"].as_str().unwrap_or_default().to_string(),
            }),
            "boom" => Err("handler-level failure".into()),
            _ => Err("unknown tool".into()),
        }
    }

    fn resources(&self) -> Vec<ResourceSpec> {
        vec![ResourceSpec {
            uri: "trove-stub://index".into(),
            name: "index".into(),
            description: "stub index".into(),
        }]
    }

    fn read_resource(&self, uri: &str) -> Result<ResourceContents, String> {
        if uri == "trove-stub://index" {
            Ok(ResourceContents {
                uri: uri.into(),
                text: "# stub".into(),
            })
        } else {
            Err(format!("unknown resource {uri}"))
        }
    }

    fn prompts(&self) -> Vec<PromptSpec> {
        vec![PromptSpec {
            name: "stub/p".into(),
            description: "stub prompt".into(),
        }]
    }

    fn get_prompt(&self, name: &str) -> Result<PromptResult, String> {
        if name == "stub/p" {
            Ok(PromptResult {
                text: "do the thing".into(),
            })
        } else {
            Err("unknown prompt".into())
        }
    }
}

fn drive(transcript: &str) -> String {
    let mut reader = Cursor::new(transcript.as_bytes().to_vec());
    let mut writer = Cursor::new(Vec::<u8>::new());
    trove_mcp::run(&mut reader, &mut writer, &Stub).unwrap();
    String::from_utf8(writer.into_inner()).unwrap()
}

fn lines(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .map(Result::unwrap)
        .collect()
}

#[test]
fn lifecycle_tools_resources_prompts() {
    let out = drive(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n\
         {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
         {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n\
         {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\"}\n\
         {\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"echo\",\"arguments\":{\"q\":\"hi\"}}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"resources/list\"}\n\
         {\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"resources/read\",\"params\":{\"uri\":\"trove-stub://index\"}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"prompts/list\"}\n\
         {\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"prompts/get\",\"params\":{\"name\":\"stub/p\"}}\n",
    );
    let l = lines(&out);
    assert_eq!(
        l.len(),
        8,
        "one response per request; notifications silent: {out}"
    );
    assert_eq!(l[0]["result"]["serverInfo"]["name"], "trove-stub");
    assert_eq!(l[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        l[1]["result"],
        serde_json::json!({}),
        "ping -> empty result"
    );
    let tools = l[2]["result"]["tools"].as_array().unwrap();
    let echo = tools
        .iter()
        .find(|t| t["name"] == "echo")
        .expect("echo advertised");
    assert!(echo["inputSchema"]["properties"]["q"].is_object());
    assert_eq!(l[3]["result"]["content"][0]["text"], "hi");
    assert_eq!(l[3]["result"]["isError"], false);
    assert_eq!(l[4]["result"]["resources"][0]["uri"], "trove-stub://index");
    assert_eq!(l[5]["result"]["contents"][0]["text"], "# stub");
    assert_eq!(l[5]["result"]["contents"][0]["mimeType"], "text/markdown");
    assert_eq!(l[6]["result"]["prompts"][0]["name"], "stub/p");
    assert_eq!(
        l[7]["result"]["messages"][0]["content"]["text"],
        "do the thing"
    );
    assert_eq!(l[7]["result"]["messages"][0]["role"], "user");
}

#[test]
fn malformed_json_is_parse_error() {
    let out = drive("this is not json\n");
    let l = lines(&out);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0]["id"], serde_json::Value::Null);
    assert_eq!(l[0]["error"]["code"], -32700);
}

#[test]
fn unknown_method_is_method_not_found() {
    let out = drive("{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"workspace/symbol\"}\n");
    let l = lines(&out);
    assert_eq!(l[0]["error"]["code"], -32601);
}

#[test]
fn unknown_tool_and_resource_are_invalid_params() {
    let out = drive(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"nope\",\"arguments\":{}}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"resources/read\",\"params\":{\"uri\":\"trove-stub://nope\"}}\n",
    );
    let l = lines(&out);
    assert_eq!(l[0]["error"]["code"], -32602, "unknown tool: {out}");
    assert_eq!(l[1]["error"]["code"], -32602, "unknown resource uri: {out}");
}

#[test]
fn handler_failure_is_tool_error_not_protocol_error() {
    let out =
        drive("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"boom\",\"arguments\":{}}}\n");
    let l = lines(&out);
    assert!(l[0]["result"].is_object(), "not a protocol error: {out}");
    assert_eq!(l[0]["result"]["isError"], true);
    assert_eq!(
        l[0]["result"]["content"][0]["text"],
        "handler-level failure"
    );
}

#[test]
fn notifications_never_get_responses() {
    let out = drive(
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
         {\"jsonrpc\":\"2.0\",\"method\":\"$/cancelRequest\",\"params\":{\"id\":99}}\n\
         {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    );
    let l = lines(&out);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0]["id"], 1);
}

#[test]
fn initialize_negotiates_protocol_version() {
    let ok_new = drive(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\"}}\n",
    );
    let l = lines(&ok_new);
    assert_eq!(l[0]["result"]["protocolVersion"], "2025-03-26");
    let ok_old = drive(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\"}}\n",
    );
    let l = lines(&ok_old);
    assert_eq!(l[0]["result"]["protocolVersion"], "2024-11-05");
    let unknown = drive(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2099-01-01\"}}\n",
    );
    let l = lines(&unknown);
    assert_eq!(l[0]["result"]["protocolVersion"], "2024-11-05");
}
