//! MCP の口(stdio、改行区切り JSON-RPC 2.0)。契約は docs/mcp_v0.md。
//! ツールは monban.declare ただ一つ — 改めの口は将来の版でも存在させない(三条二)。

use crate::{declare_claim, Ws};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const TOOL_DECLARE: &str = "monban.declare";
const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn serve(ws: &Ws) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin を読めない: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let response = match method {
            "initialize" => {
                let requested = msg
                    .pointer("/params/protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or(PROTOCOL_VERSION);
                Some(result(
                    id,
                    json!({
                        "protocolVersion": requested,
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "monban", "version": env!("CARGO_PKG_VERSION") },
                    }),
                ))
            }
            "ping" => Some(result(id, json!({}))),
            "tools/list" => Some(result(id, json!({ "tools": [tool_schema()] }))),
            "tools/call" => Some(handle_call(ws, id, msg.get("params"))),
            _ if id.is_some() => Some(error(id, -32601, &format!("method not found: {method}"))),
            _ => None, // notifications/initialized 等の通知には応答しない
        };
        if let Some(r) = response {
            writeln!(out, "{r}").map_err(|e| format!("stdout に書けない: {e}"))?;
            out.flush().map_err(|e| format!("stdout に書けない: {e}"))?;
        }
    }
    Ok(())
}

fn tool_schema() -> Value {
    json!({
        "name": TOOL_DECLARE,
        "description": "完了を主張し、成果物そのものを証拠として提出する(唯一の入口)。\
            証拠なしの主張は通らない。改め(verify)はこの口には無い — 門番の仕事である。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "seki": { "type": "string", "description": "通ろうとする関の名前(monban.toml に実在)" },
                "title": { "type": "string", "description": "主張の一文" },
                "evidence": {
                    "type": "array", "items": { "type": "string" }, "minItems": 1,
                    "description": "成果物ファイルのパス(1件以上必須)"
                },
                "actor": { "type": "string", "description": "宣言者の名。agent: で始まること" },
            },
            "required": ["seki", "title", "evidence", "actor"],
        },
    })
}

fn handle_call(ws: &Ws, id: Option<Value>, params: Option<&Value>) -> Value {
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if name != TOOL_DECLARE {
        return error(
            id,
            -32602,
            &format!("no such tool: {name}(この口の道具は {TOOL_DECLARE} だけ。改めは門番の仕事)"),
        );
    }
    let args = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or(json!({}));
    match call_declare(ws, &args) {
        Ok(text) => result(
            id,
            json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
        ),
        Err(msg) => result(
            id,
            json!({ "content": [{ "type": "text", "text": format!("止まる — {msg}") }], "isError": true }),
        ),
    }
}

fn call_declare(ws: &Ws, args: &Value) -> Result<String, String> {
    let s = |key: &str| -> Result<String, String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| format!("引数 {key} が無い"))
    };
    let seki = s("seki")?;
    let title = s("title")?;
    let actor = s("actor")?;
    if !actor.starts_with("agent:") {
        return Err(format!(
            "この口はエージェント向け。actor は agent: で始まること(実際: \"{actor}\")"
        ));
    }
    let evidence: Vec<PathBuf> = args
        .get("evidence")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default();
    let event = declare_claim(ws, &seki, &title, &evidence, actor)?;
    Ok(format!(
        "✓ 記録 seq={} type=claim.declare id={}\n\
         改めは門番の仕事であり、この口からは呼べない。主人に ID を報せよ。",
        event.seq, event.id
    ))
}

fn result(id: Option<Value>, value: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": value })
}

fn error(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
