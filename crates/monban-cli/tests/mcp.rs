//! MCP の口の統合テスト。docs/mcp_v0.md の凍結内容 — ツールは monban.declare だけ、
//! actor は agent: 必須、改めの口は無い — を実プロセスの stdio で検める。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Ws(PathBuf);

impl Drop for Ws {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn workspace() -> Ws {
    let dir = std::env::temp_dir().join(format!(
        "monban-mcp-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ledger")).unwrap();
    std::fs::write(dir.join("banto.toml"), "actor = \"tester\"\n").unwrap();
    banto_kernel::Ledger::create(&dir.join("ledger/ledger.jsonl")).unwrap();
    std::fs::write(
        dir.join("monban.toml"),
        "schema = \"monban/0\"\n\n[[seki]]\nname = \"tests-pass\"\ntitle = \"テスト\"\n\n  [[seki.evidence]]\n  kind = \"toolchain\"\n  cmd = [\"true\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("seika.md"), "成果物\n").unwrap();
    Ws(dir)
}

/// JSON-RPC 行を流し込み、応答行(JSON)を id で引けるようにして返す。
fn session(root: &Path, requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("monban"))
        .arg("mcp")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for req in requests {
            writeln!(stdin, "{req}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn init() -> serde_json::Value {
    serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize",
        "params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}})
}

fn call(id: u64, name: &str, args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":name,"arguments":args}})
}

fn events(root: &Path) -> Vec<banto_kernel::Envelope> {
    banto_kernel::Ledger::open(&root.join("ledger/ledger.jsonl"))
        .unwrap()
        .events()
        .unwrap()
}

#[test]
fn lists_exactly_one_tool() {
    let ws = workspace();
    let res = session(
        &ws.0,
        &[
            init(),
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        ],
    );
    let tools = &res[1]["result"]["tools"];
    assert_eq!(tools.as_array().unwrap().len(), 1, "道具は一つだけ");
    assert_eq!(tools[0]["name"], "monban.declare");
}

#[test]
fn declare_via_mcp_appends_claim() {
    let ws = workspace();
    let res = session(
        &ws.0,
        &[
            init(),
            call(
                1,
                "monban.declare",
                serde_json::json!({
                "seki": "tests-pass", "title": "テストが通りました",
                "evidence": ["seika.md"], "actor": "agent:claude"}),
            ),
        ],
    );
    assert_eq!(res[1]["result"]["isError"], false, "{}", res[1]);
    let text = res[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("id="));
    let ev = events(&ws.0);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].event_type, "claim.declare");
    assert_eq!(ev[0].actor, "agent:claude");
}

#[test]
fn declare_without_evidence_stops() {
    let ws = workspace();
    let res = session(
        &ws.0,
        &[
            init(),
            call(
                1,
                "monban.declare",
                serde_json::json!({
                "seki": "tests-pass", "title": "できました",
                "evidence": [], "actor": "agent:claude"}),
            ),
        ],
    );
    assert_eq!(res[1]["result"]["isError"], true);
    assert!(res[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("止まる"));
    assert!(events(&ws.0).is_empty(), "拒否された宣言は台帳に残らない");
}

#[test]
fn non_agent_actor_is_refused() {
    let ws = workspace();
    let res = session(
        &ws.0,
        &[
            init(),
            call(
                1,
                "monban.declare",
                serde_json::json!({
                "seki": "tests-pass", "title": "x",
                "evidence": ["seika.md"], "actor": "keisuke"}),
            ),
        ],
    );
    assert_eq!(res[1]["result"]["isError"], true);
    assert!(events(&ws.0).is_empty());
}

#[test]
fn gate_actor_is_refused() {
    let ws = workspace();
    let res = session(
        &ws.0,
        &[
            init(),
            call(
                1,
                "monban.declare",
                serde_json::json!({
                "seki": "tests-pass", "title": "x",
                "evidence": ["seika.md"], "actor": "agent:monban"}),
            ),
        ],
    );
    assert_eq!(res[1]["result"]["isError"], true);
    assert!(events(&ws.0).is_empty());
}

#[test]
fn non_string_evidence_element_is_refused() {
    // 型の合わない要素を黙って捨てると証拠が細る — 明示に止まる(三条一)
    let ws = workspace();
    let res = session(
        &ws.0,
        &[
            init(),
            call(
                1,
                "monban.declare",
                serde_json::json!({
                "seki": "tests-pass", "title": "x",
                "evidence": ["seika.md", 42], "actor": "agent:claude"}),
            ),
        ],
    );
    assert_eq!(res[1]["result"]["isError"], true, "{}", res[1]);
    assert!(res[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("文字列ではない"));
    assert!(events(&ws.0).is_empty());
}

#[test]
fn mcp_declare_then_otel_verify_roundtrip() {
    // MCP で宣言 → CLI の改めで otel 検分が pass する往復。
    // body.artifacts のラベルと写しの対応が MCP 経由でも成立することを固定する
    let ws = workspace();
    std::fs::write(
        ws.0.join("monban.toml"),
        "schema = \"monban/0\"\n\n[[seki]]\nname = \"report-honest\"\ntitle = \"裏付け\"\n\n  [[seki.evidence]]\n  kind = \"otel\"\n  vault = \"vault.jsonl\"\n  span = \"read_sources\"\n  attrs_match = [\"lines.total\"]\n",
    )
    .unwrap();
    let line = serde_json::json!({"resourceSpans": [{"scopeSpans": [{"spans": [{
        "name": "read_sources",
        "attributes": [{"key": "lines.total", "value": {"intValue": "4106"}}]
    }]}]}]})
    .to_string();
    std::fs::write(ws.0.join("vault.jsonl"), line + "\n").unwrap();
    std::fs::write(ws.0.join("seika.md"), "合計: 4106 行\n").unwrap();
    let res = session(
        &ws.0,
        &[
            init(),
            call(
                1,
                "monban.declare",
                serde_json::json!({
                "seki": "report-honest", "title": "レポートを書いた",
                "evidence": ["seika.md"], "actor": "agent:claude"}),
            ),
        ],
    );
    assert_eq!(res[1]["result"]["isError"], false, "{}", res[1]);
    let text = res[1]["result"]["content"][0]["text"].as_str().unwrap();
    let id = text
        .split("id=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    let out = std::process::Command::new(assert_cmd::cargo::cargo_bin("monban"))
        .args(["verify", id])
        .current_dir(&ws.0)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.body["verdict"], "pass");
    assert_eq!(verify.body["checks"][0]["kind"], "otel");
}

#[test]
fn there_is_no_verify_tool() {
    let ws = workspace();
    let res = session(
        &ws.0,
        &[
            init(),
            call(1, "monban.verify", serde_json::json!({"claim": "x"})),
        ],
    );
    assert!(
        res[1].get("error").is_some(),
        "改めの口は存在しない: {}",
        res[1]
    );
    assert!(events(&ws.0).is_empty());
}
