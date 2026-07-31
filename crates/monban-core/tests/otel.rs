//! OTel 検分の芯の単体検査。規則の権威は docs/otel_evidence_v0.md —
//! ここはその規則が一つずつ機械で守られることを確かめる。

use monban_core::examine_otel;
use std::path::PathBuf;

struct Vault(PathBuf);

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn vault(lines: &[String]) -> Vault {
    let path = std::env::temp_dir().join(format!(
        "monban-otel-{}-{:?}.jsonl",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    Vault(path)
}

/// OTLP/JSON 1行(ExportTraceServiceRequest)。value は OTLP の AnyValue をそのまま渡す。
fn otlp_line(span: &str, attrs: &[(&str, serde_json::Value)]) -> String {
    let attributes: Vec<_> = attrs
        .iter()
        .map(|(k, v)| serde_json::json!({"key": k, "value": v}))
        .collect();
    serde_json::json!({
        "resourceSpans": [{
            "resource": {"attributes": []},
            "scopeSpans": [{
                "scope": {"name": "test"},
                "spans": [{"name": span, "attributes": attributes}]
            }]
        }]
    })
    .to_string()
}

fn art(text: &str) -> Vec<(String, String)> {
    vec![("report.md".into(), text.into())]
}

#[test]
fn honest_span_matches_artifact() {
    let v = vault(&[otlp_line(
        "read_sources",
        &[("lines.total", serde_json::json!({"intValue": "4106"}))],
    )]);
    let attrs = vec!["lines.total".to_string()];
    let run = examine_otel(&v.0, "read_sources", &attrs, Some(&art("合計: 4106 行\n")));
    assert!(run.ok, "{}", run.transcript);
    assert_eq!(run.spans_found, 1);
}

#[test]
fn absent_span_does_not_pass() {
    let v = vault(&[otlp_line("other", &[])]);
    let run = examine_otel(&v.0, "read_sources", &[], Some(&art("x")));
    assert!(!run.ok);
    assert_eq!(run.spans_found, 0);
}

#[test]
fn missing_vault_does_not_pass() {
    let run = examine_otel(
        std::path::Path::new("/nonexistent/vault.jsonl"),
        "s",
        &[],
        Some(&art("x")),
    );
    assert!(!run.ok);
    assert!(run.transcript.contains("蔵を読めない"));
}

#[test]
fn malformed_vault_line_does_not_pass() {
    let v = vault(&["not json {".to_string()]);
    let run = examine_otel(&v.0, "s", &[], Some(&art("x")));
    assert!(!run.ok);
    assert!(run.transcript.contains("解析できない"));
}

#[test]
fn value_absent_from_artifact_does_not_pass() {
    let v = vault(&[otlp_line(
        "read_sources",
        &[("lines.total", serde_json::json!({"intValue": "5000"}))],
    )]);
    let attrs = vec!["lines.total".to_string()];
    let run = examine_otel(&v.0, "read_sources", &attrs, Some(&art("合計: 4106 行\n")));
    assert!(!run.ok);
    assert!(run.transcript.contains("5000"));
}

/// demo2 第二幕: 改竄の再送は追記され、矛盾する二本目が通り道を塞ぐ。
#[test]
fn tampered_resend_blocks_its_own_path() {
    let honest = otlp_line(
        "read_sources",
        &[("lines.total", serde_json::json!({"intValue": "4106"}))],
    );
    let tampered = otlp_line(
        "read_sources",
        &[("lines.total", serde_json::json!({"intValue": "9999"}))],
    );
    let v = vault(&[honest, tampered]);
    let attrs = vec!["lines.total".to_string()];
    // 成果物がどちらの数字でも、他方の span が矛盾して fail
    for report in ["合計: 4106 行", "合計: 9999 行"] {
        let run = examine_otel(&v.0, "read_sources", &attrs, Some(&art(report)));
        assert!(!run.ok, "成果物 {report}: {}", run.transcript);
        assert_eq!(run.spans_found, 2);
    }
}

#[test]
fn attr_missing_on_a_span_does_not_pass() {
    let v = vault(&[otlp_line(
        "s",
        &[("other", serde_json::json!({"stringValue": "x"}))],
    )]);
    let attrs = vec!["lines.total".to_string()];
    let run = examine_otel(&v.0, "s", &attrs, Some(&art("x")));
    assert!(!run.ok);
    assert!(run.transcript.contains("属性が無い"));
}

#[test]
fn empty_value_does_not_pass() {
    let v = vault(&[otlp_line(
        "s",
        &[("a", serde_json::json!({"stringValue": ""}))],
    )]);
    let attrs = vec!["a".to_string()];
    let run = examine_otel(&v.0, "s", &attrs, Some(&art("anything")));
    assert!(!run.ok);
    assert!(run.transcript.contains("空"));
}

#[test]
fn non_scalar_value_does_not_pass() {
    let v = vault(&[otlp_line(
        "s",
        &[("a", serde_json::json!({"arrayValue": {"values": []}}))],
    )]);
    let attrs = vec!["a".to_string()];
    let run = examine_otel(&v.0, "s", &attrs, Some(&art("anything")));
    assert!(!run.ok);
    assert!(run.transcript.contains("非スカラー"));
}

#[test]
fn scalar_canonicalization_is_lenient_about_json_encoding() {
    // intValue は仕様上 JSON 文字列だが、数値で書く実装にも応じる。double・bool も通る。
    let v = vault(&[otlp_line(
        "s",
        &[
            ("i", serde_json::json!({"intValue": 13})),
            ("d", serde_json::json!({"doubleValue": 1.5})),
            ("b", serde_json::json!({"boolValue": true})),
        ],
    )]);
    let attrs = vec!["i".to_string(), "d".to_string(), "b".to_string()];
    let run = examine_otel(&v.0, "s", &attrs, Some(&art("13 件、1.5 秒、true\n")));
    assert!(run.ok, "{}", run.transcript);
}

#[test]
fn no_attrs_match_means_presence_only() {
    let v = vault(&[otlp_line("s", &[])]);
    let run = examine_otel(&v.0, "s", &[], Some(&art("x")));
    assert!(run.ok);
}

#[test]
fn dry_run_checks_presence_and_defers_matching() {
    let v = vault(&[otlp_line(
        "s",
        &[(
            "a",
            serde_json::json!({"stringValue": "not-in-any-artifact"}),
        )],
    )]);
    let attrs = vec!["a".to_string()];
    // 予行(artifacts なし): span が在れば ok。attrs_match は改めに委ねる
    let run = examine_otel(&v.0, "s", &attrs, None);
    assert!(run.ok, "{}", run.transcript);
    assert!(run.transcript.contains("予行"));
}

#[test]
fn spans_are_found_across_lines_and_resource_groups() {
    let two_groups = serde_json::json!({
        "resourceSpans": [
            {"scopeSpans": [{"spans": [{"name": "s", "attributes":
                [{"key": "a", "value": {"stringValue": "v1"}}]}]}]},
            {"scopeSpans": [{"spans": [{"name": "s", "attributes":
                [{"key": "a", "value": {"stringValue": "v2"}}]}]}]}
        ]
    })
    .to_string();
    let v = vault(&[
        two_groups,
        otlp_line("s", &[("a", serde_json::json!({"stringValue": "v3"}))]),
    ]);
    let attrs = vec!["a".to_string()];
    let run = examine_otel(&v.0, "s", &attrs, Some(&art("v1 v2 v3")));
    assert!(run.ok, "{}", run.transcript);
    assert_eq!(run.spans_found, 3);
}
