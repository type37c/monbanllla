//! 門の統合テスト。三条(証拠なしは通らない/宣言者は自分を改められない/追記の鎖)が
//! 実際に機械執行されることを、実バイナリで確かめる。

use assert_cmd::Command;
use std::path::{Path, PathBuf};

struct Ws(PathBuf);

impl Drop for Ws {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// banto ワークスペース(banto.toml + 空台帳)+ 契約を据えた仮の作業場を作る。
fn workspace(contract: &str) -> Ws {
    let dir = std::env::temp_dir().join(format!(
        "monban-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ledger")).unwrap();
    std::fs::write(dir.join("banto.toml"), "actor = \"tester\"\n").unwrap();
    banto_kernel::Ledger::create(&dir.join("ledger/ledger.jsonl")).unwrap();
    std::fs::write(dir.join("monban.toml"), contract).unwrap();
    Ws(dir)
}

fn monban(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("monban").unwrap();
    cmd.current_dir(root);
    cmd
}

fn events(root: &Path) -> Vec<banto_kernel::Envelope> {
    banto_kernel::Ledger::open(&root.join("ledger/ledger.jsonl"))
        .unwrap()
        .events()
        .unwrap()
}

const PASS_CONTRACT: &str = r#"
schema = "monban/0"

[[seki]]
name = "tests-pass"
title = "テストが通っている"

  [[seki.evidence]]
  kind = "toolchain"
  cmd = ["true"]
"#;

const FAIL_CONTRACT: &str = r#"
schema = "monban/0"

[[seki]]
name = "tests-pass"
title = "テストが通っている"

  [[seki.evidence]]
  kind = "toolchain"
  cmd = ["false"]
"#;

fn declare(root: &Path) -> String {
    std::fs::write(root.join("seika.md"), "成果物\n").unwrap();
    let out = monban(root)
        .args([
            "declare",
            "tests-pass",
            "テストが通りました",
            "--evidence",
            "seika.md",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let id = stdout
        .split("id=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    id.to_string()
}

// ---- 契約の読み込み(執行規則1: 証拠のない関はロード時拒否) ----

#[test]
fn contract_without_evidence_is_rejected() {
    let ws = workspace("schema = \"monban/0\"\n[[seki]]\nname = \"empty\"\ntitle = \"証拠なし\"\n");
    monban(&ws.0)
        .arg("contract")
        .assert()
        .failure()
        .stderr(predicates::str::contains("門ではない"));
}

#[test]
fn otel_kind_is_reserved_in_v01() {
    let ws = workspace(
        "schema = \"monban/0\"\n[[seki]]\nname = \"o\"\ntitle = \"otel\"\n[[seki.evidence]]\nkind = \"otel\"\n",
    );
    monban(&ws.0)
        .arg("contract")
        .assert()
        .failure()
        .stderr(predicates::str::contains("未実装"));
}

#[test]
fn wrong_schema_is_rejected() {
    let ws = workspace("schema = \"monban/9\"\n");
    monban(&ws.0)
        .arg("contract")
        .assert()
        .failure()
        .stderr(predicates::str::contains("monban/0"));
}

// ---- 三条一: 証拠なしの主張は通らない ----

#[test]
fn declare_without_evidence_is_rejected() {
    let ws = workspace(PASS_CONTRACT);
    monban(&ws.0)
        .args(["declare", "tests-pass", "できました"])
        .assert()
        .failure();
    assert!(events(&ws.0).is_empty(), "拒否された宣言は台帳に残らない");
}

#[test]
fn declare_binds_artifact_and_contract_hash() {
    let ws = workspace(PASS_CONTRACT);
    declare(&ws.0);
    let ev = events(&ws.0);
    assert_eq!(ev.len(), 1);
    let claim = &ev[0];
    assert_eq!(claim.event_type, "claim.declare");
    assert_eq!(claim.body["seki"], "tests-pass");
    // 成果物 + 契約原文の写しの2件。どちらも蔵に納まっている
    assert_eq!(claim.evidence.len(), 2);
    for e in &claim.evidence {
        assert!(ws.0.join("ledger/objects").join(&e.sha256).is_file());
    }
    // body.contract は今の monban.toml のハッシュと一致する(執行規則2)
    let contract_raw = std::fs::read(ws.0.join("monban.toml")).unwrap();
    assert_eq!(
        claim.body["contract"],
        serde_json::json!(banto_kernel::hash_bytes(&contract_raw))
    );
}

// ---- 三条二: 宣言者は自分を改められない ----

#[test]
fn gate_actor_cannot_declare() {
    let ws = workspace(PASS_CONTRACT);
    std::fs::write(ws.0.join("seika.md"), "x\n").unwrap();
    monban(&ws.0)
        .args([
            "declare",
            "tests-pass",
            "x",
            "--evidence",
            "seika.md",
            "--actor",
            "agent:monban",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("門番"));
}

#[test]
fn gate_refuses_to_verify_its_own_declaration() {
    let ws = workspace(PASS_CONTRACT);
    // 汎用の口(banto-kernel 直書き)で門番名義の宣言を偽造しても、改めは拒まれる
    let ledger = banto_kernel::Ledger::open(&ws.0.join("ledger/ledger.jsonl")).unwrap();
    let ev = ledger
        .append(banto_kernel::Draft {
            ts: None,
            actor: "agent:monban".into(),
            event_type: "claim.declare".into(),
            body: serde_json::json!({"title": "x", "seki": "tests-pass"}),
            evidence: vec![banto_kernel::Evidence {
                sha256: banto_kernel::hash_bytes(b"x"),
                uri: None,
                media_type: None,
                bytes: None,
            }],
            op: None,
        })
        .unwrap();
    monban(&ws.0)
        .args(["verify", &ev.id])
        .assert()
        .failure()
        .stderr(predicates::str::contains("改められない"));
    assert_eq!(events(&ws.0).len(), 1, "拒否は台帳に判定を残さない");
}

// ---- 改め: 門番自身が検査を走らせる ----

#[test]
fn verify_pass_issues_tegata() {
    let ws = workspace(PASS_CONTRACT);
    let id = declare(&ws.0);
    monban(&ws.0)
        .args(["verify", &id])
        .assert()
        .success()
        .stdout(predicates::str::contains("手形"));
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.event_type, "claim.verify");
    assert_eq!(verify.actor, "agent:monban");
    assert_eq!(verify.body["verdict"], "pass");
    assert_eq!(verify.body["claim"], serde_json::json!(id));
    // 検査の証跡が蔵に納まっている
    assert!(!verify.evidence.is_empty());
    assert!(ws
        .0
        .join("ledger/objects")
        .join(&verify.evidence[0].sha256)
        .is_file());
}

#[test]
fn verify_fail_stops_at_the_gate() {
    let ws = workspace(FAIL_CONTRACT);
    let id = declare(&ws.0);
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("止まる"));
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.body["verdict"], "fail");
}

// ---- 執行規則2: 契約の差し替えは台帳に写る ----

#[test]
fn contract_swap_after_declare_fails_verification() {
    let ws = workspace(FAIL_CONTRACT);
    let id = declare(&ws.0);
    // 宣言のあとで契約を通りやすいものに差し替える(門の説得の試み)
    std::fs::write(ws.0.join("monban.toml"), PASS_CONTRACT).unwrap();
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.body["verdict"], "fail");
    assert!(verify.body["reason"]
        .as_str()
        .unwrap()
        .contains("差し替わっている"));
}

// ---- 予行(check)は台帳に書かない ----

#[test]
fn check_does_not_touch_the_ledger() {
    let ws = workspace(PASS_CONTRACT);
    monban(&ws.0).arg("check").assert().success();
    assert!(events(&ws.0).is_empty());
}

#[test]
fn check_reports_failure() {
    let ws = workspace(FAIL_CONTRACT);
    monban(&ws.0).arg("check").assert().code(1);
}

// ---- 台帳の鎖は banto verify 相当の検査に通る ----

#[test]
fn ledger_chain_stays_healthy() {
    let ws = workspace(PASS_CONTRACT);
    let id = declare(&ws.0);
    monban(&ws.0).args(["verify", &id]).assert().success();
    let ledger = banto_kernel::Ledger::open(&ws.0.join("ledger/ledger.jsonl")).unwrap();
    let report = ledger.verify(Some(&ws.0), true).unwrap();
    assert!(report.ok(), "{:?}", report);
}
