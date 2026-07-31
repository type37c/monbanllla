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
    declare_with(root, "tests-pass", "成果物\n")
}

fn declare_with(root: &Path, seki: &str, content: &str) -> String {
    std::fs::write(root.join("seika.md"), content).unwrap();
    let out = monban(root)
        .args(["declare", seki, "できました", "--evidence", "seika.md"])
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
fn otel_without_vault_or_span_is_rejected() {
    let ws = workspace(
        "schema = \"monban/0\"\n[[seki]]\nname = \"o\"\ntitle = \"otel\"\n[[seki.evidence]]\nkind = \"otel\"\n",
    );
    monban(&ws.0)
        .arg("contract")
        .assert()
        .failure()
        .stderr(predicates::str::contains("非空"));
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

// ---- OTel 証拠(v0.3): 蔵の span と宣言時の成果物写しの突き合わせ ----

const OTEL_CONTRACT: &str = r#"
schema = "monban/0"

[[seki]]
name = "report-honest"
title = "レポートの数字は実行過程に裏付けがある"

  [[seki.evidence]]
  kind = "otel"
  vault = "vault.jsonl"
  span = "read_sources"
  attrs_match = ["lines.total"]
"#;

fn write_vault(root: &Path, totals: &[&str]) {
    let lines: Vec<String> = totals
        .iter()
        .map(|t| {
            serde_json::json!({"resourceSpans": [{"scopeSpans": [{"spans": [{
                "name": "read_sources",
                "attributes": [{"key": "lines.total", "value": {"intValue": t}}]
            }]}]}]})
            .to_string()
        })
        .collect();
    std::fs::write(root.join("vault.jsonl"), lines.join("\n") + "\n").unwrap();
}

#[test]
fn otel_contract_is_listed() {
    let ws = workspace(OTEL_CONTRACT);
    monban(&ws.0)
        .arg("contract")
        .assert()
        .success()
        .stdout(predicates::str::contains("otel: span \"read_sources\""));
}

#[test]
fn otel_verify_pass_issues_tegata() {
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["4106"]);
    let id = declare_with(&ws.0, "report-honest", "合計: 4106 行(13 ファイル)\n");
    monban(&ws.0)
        .args(["verify", &id])
        .assert()
        .success()
        .stdout(predicates::str::contains("手形"));
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.body["verdict"], "pass");
    assert_eq!(verify.body["checks"][0]["kind"], "otel");
    assert_eq!(verify.body["checks"][0]["spans_found"], 1);
    // 検分の証跡(transcript)が蔵に納まっている
    assert!(!verify.evidence.is_empty());
}

#[test]
fn otel_verify_fails_when_report_disagrees_with_vault() {
    // demo2 第四幕の変形: 蔵は 4106 を証言、レポートは 9999 を主張
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["4106"]);
    let id = declare_with(&ws.0, "report-honest", "合計: 9999 行\n");
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(events(&ws.0).last().unwrap().body["verdict"], "fail");
}

#[test]
fn otel_verify_fails_on_tampered_resend() {
    // demo2 第二幕: 追記専用の蔵では、矛盾する再送が自分の通り道を塞ぐ
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["4106", "9999"]);
    let id = declare_with(&ws.0, "report-honest", "合計: 9999 行\n");
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(events(&ws.0).last().unwrap().body["verdict"], "fail");
}

#[test]
fn otel_verify_fails_when_span_is_absent() {
    let ws = workspace(OTEL_CONTRACT);
    std::fs::write(ws.0.join("vault.jsonl"), "").unwrap();
    let id = declare_with(&ws.0, "report-honest", "合計: 4106 行\n");
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn otel_matches_declared_copy_not_working_tree() {
    // 宣言後に成果物を書き替えても、突き合わせ先は蔵(objects/)の写しのまま
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["4106"]);
    let id = declare_with(&ws.0, "report-honest", "合計: 4106 行\n");
    std::fs::write(ws.0.join("seika.md"), "合計: 9999 行(改竄)\n").unwrap();
    monban(&ws.0).args(["verify", &id]).assert().success();
}

#[test]
fn otel_verify_fails_when_declared_copy_is_lost() {
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["4106"]);
    let id = declare_with(&ws.0, "report-honest", "合計: 4106 行\n");
    // 蔵から成果物の写しを消す(7/17 の証拠消失の再現)
    let claim = events(&ws.0).into_iter().next().unwrap();
    std::fs::remove_file(ws.0.join("ledger/objects").join(&claim.evidence[0].sha256)).unwrap();
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.body["verdict"], "fail");
    assert!(verify.body["reason"].as_str().unwrap().contains("写し"));
}

#[test]
fn otel_and_toolchain_combine_as_and() {
    // 「やったか」は otel、「動くか」は toolchain — 同じ関に AND で並ぶ
    let contract = r#"
schema = "monban/0"

[[seki]]
name = "report-honest"
title = "裏付けがあり、かつ検査が通る"

  [[seki.evidence]]
  kind = "otel"
  vault = "vault.jsonl"
  span = "read_sources"
  attrs_match = ["lines.total"]

  [[seki.evidence]]
  kind = "toolchain"
  cmd = ["false"]
"#;
    let ws = workspace(contract);
    write_vault(&ws.0, &["4106"]);
    let id = declare_with(&ws.0, "report-honest", "合計: 4106 行\n");
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "otel が通っても toolchain が止める"
    );
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.body["checks"].as_array().unwrap().len(), 2);
    assert_eq!(verify.body["checks"][0]["ok"], true);
    assert_eq!(verify.body["checks"][1]["ok"], false);
}

#[test]
fn otel_without_attrs_match_is_rejected_at_load() {
    // 在否だけの otel は成果物と結ばれない — stub span 一本で通る関は門ではない
    let contract = r#"
schema = "monban/0"

[[seki]]
name = "o"
title = "在否のみ"

  [[seki.evidence]]
  kind = "otel"
  vault = "vault.jsonl"
  span = "s"
"#;
    let ws = workspace(contract);
    monban(&ws.0)
        .arg("contract")
        .assert()
        .failure()
        .stderr(predicates::str::contains("attrs_match"));
}

#[test]
fn otel_verify_fails_when_declared_copy_is_corrupted() {
    // objects/ の写しを、目撃値を仕込んだ別内容で上書きする(蔵への細工)。
    // 再ハッシュ検査が最後の防壁 — ファイル名の sha256 を信じない
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["4106"]);
    let id = declare_with(&ws.0, "report-honest", "合計: 9999 行\n");
    let claim = events(&ws.0).into_iter().next().unwrap();
    std::fs::write(
        ws.0.join("ledger/objects").join(&claim.evidence[0].sha256),
        "合計: 4106 行(すり替え)\n",
    )
    .unwrap();
    let out = monban(&ws.0).args(["verify", &id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let ev = events(&ws.0);
    let verify = ev.last().unwrap();
    assert_eq!(verify.body["verdict"], "fail");
    assert!(verify.body["reason"].as_str().unwrap().contains("合わない"));
}

#[test]
fn otel_verify_fails_for_foreign_declare_without_artifacts() {
    // monban 以外の口(kernel 直書き等)から入った、body.artifacts の無い宣言。
    // 突き合わせる成果物写しがゼロ → attrs_match は何にも一致できず fail(閉じて落ちる)
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["4106"]);
    let ledger = banto_kernel::Ledger::open(&ws.0.join("ledger/ledger.jsonl")).unwrap();
    let ev = ledger
        .append(banto_kernel::Draft {
            ts: None,
            actor: "tester".into(),
            event_type: "claim.declare".into(),
            body: serde_json::json!({"title": "x", "seki": "report-honest"}),
            evidence: vec![banto_kernel::Evidence {
                sha256: banto_kernel::hash_bytes(b"x"),
                uri: None,
                media_type: None,
                bytes: None,
            }],
            op: None,
        })
        .unwrap();
    let out = monban(&ws.0).args(["verify", &ev.id]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let events_after = events(&ws.0);
    let verify = events_after.last().unwrap();
    assert_eq!(verify.body["verdict"], "fail");
}

#[test]
fn otel_check_is_presence_only_and_writes_nothing() {
    let ws = workspace(OTEL_CONTRACT);
    write_vault(&ws.0, &["only-in-vault-not-in-any-artifact"]);
    // 予行: span が在れば ok(attrs_match は宣言の成果物が要るため改めに委ねる)
    monban(&ws.0).arg("check").assert().success();
    assert!(events(&ws.0).is_empty());
    // span が無ければ予行でも止まる
    std::fs::write(ws.0.join("vault.jsonl"), "").unwrap();
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
