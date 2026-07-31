//! OTel 証拠の検分。蔵は独立コレクタの受領記録 — OTel Collector の file exporter が
//! 吐く OTLP/JSON 行(1行 = ExportTraceServiceRequest)をそのまま読む。器は標準を借りる。
//!
//! 規則の権威は docs/otel_evidence_v0.md。要点:
//! 名指しの span が無ければ通らない。あれば**同名の全 span** の attrs_match 各属性が
//! 宣言時の成果物写しに現れなければ通らない — 追記専用の蔵では、改竄の再送は
//! 矛盾する二本目として残り、自分の通り道を自分で塞ぐ。

use std::path::Path;

/// 1回の OTel 検分の証跡。transcript がそのまま証拠ファイルになる。
#[derive(Debug)]
pub struct OtelRun {
    pub vault: String,
    pub span: String,
    pub spans_found: usize,
    pub ok: bool,
    pub transcript: String,
}

/// 検分の芯。`artifacts` は宣言時に蔵(objects/)へ写された成果物の(ラベル, 本文)。
/// None は予行(check)— attrs_match は宣言の成果物が要るため、span の在否までを見る。
/// 壊れた蔵・読めない蔵は誤りではなく「通らない」(ok = false)として証跡に残す。
pub fn examine_otel(
    vault_path: &Path,
    span_name: &str,
    attrs_match: &[String],
    artifacts: Option<&[(String, String)]>,
) -> OtelRun {
    let mut t = format!(
        "otel 検分: 蔵 {} / span \"{span_name}\"\n",
        vault_path.display()
    );
    let done = |ok: bool, spans_found: usize, transcript: String| OtelRun {
        vault: vault_path.display().to_string(),
        span: span_name.to_string(),
        spans_found,
        ok,
        transcript,
    };

    let raw = match std::fs::read_to_string(vault_path) {
        Ok(r) => r,
        Err(e) => {
            t.push_str(&format!("✗ 蔵を読めない: {e}\n"));
            return done(false, 0, t);
        }
    };

    // 蔵の全行から、名指しの span を(出現順に)集める。行の欠損は蔵の完全性の疑いなので通さない。
    let mut spans: Vec<Vec<(String, Option<String>)>> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                t.push_str(&format!("✗ 蔵の行 {} を解析できない: {e}\n", i + 1));
                return done(false, 0, t);
            }
        };
        if let Err(why) = collect_spans(&v, span_name, &mut spans) {
            t.push_str(&format!(
                "✗ 蔵の行 {} が ExportTraceServiceRequest の形ではない: {why}\n",
                i + 1
            ));
            return done(false, 0, t);
        }
    }
    t.push_str(&format!("span \"{span_name}\": {} 本\n", spans.len()));
    if spans.is_empty() {
        t.push_str("✗ 名指しの span が蔵に無い(過程の証言が無い)\n");
        return done(false, 0, t);
    }

    let Some(artifacts) = artifacts else {
        t.push_str("(予行: attrs_match の突き合わせは宣言の成果物が要るため、改めで行う)\n");
        return done(true, spans.len(), t);
    };

    let mut ok = true;
    for (si, attrs) in spans.iter().enumerate() {
        for name in attrs_match {
            let verdict = match attrs.iter().find(|(k, _)| k == name) {
                None => Err("属性が無い".to_string()),
                Some((_, None)) => Err("属性値が非スカラー(v0.3 はスカラーのみ)".into()),
                Some((_, Some(v))) if v.is_empty() => Err("属性値が空(空は突き合わせ不能)".into()),
                Some((_, Some(v))) => match artifacts.iter().find(|(_, text)| text.contains(v)) {
                    Some((label, text)) => Ok((v.clone(), label.clone(), quote_match(text, v))),
                    None => Err(format!("値 \"{v}\" がどの成果物写しにも現れない")),
                },
            };
            match verdict {
                Ok((v, label, quote)) => {
                    // 一致箇所の行を証跡に引用する — 部分文字列一致は「目撃値が成果物の
                    // どこかに在る」ことしか言えないため、どこで一致したかを人の目に届ける。
                    t.push_str(&format!(
                        "✓ span[{si}] {name} = \"{v}\" — 成果物 {label} に一致: 「{quote}」\n"
                    ));
                }
                Err(why) => {
                    t.push_str(&format!("✗ span[{si}] {name}: {why}\n"));
                    ok = false;
                }
            }
        }
    }
    if attrs_match.is_empty() {
        t.push_str("(attrs_match 指定なし — span の在否のみ)\n");
    }
    done(ok, spans.len(), t)
}

/// 一致箇所を含む行を引用用に切り出す(長い行は一致値の前後で刈り込む)。
fn quote_match(text: &str, value: &str) -> String {
    let line = text
        .lines()
        .find(|l| l.contains(value))
        .unwrap_or(value)
        .trim();
    if line.chars().count() <= 120 {
        return line.to_string();
    }
    let pos = line.find(value).unwrap_or(0);
    let start = line
        .char_indices()
        .map(|(i, _)| i)
        .rfind(|&i| i <= pos.saturating_sub(40))
        .unwrap_or(0);
    let tail: String = line[start..].chars().take(120).collect();
    format!("…{tail}…")
}

/// OTLP/JSON の resourceSpans[].scopeSpans[].spans[] から名前の合う span の属性を集める。
/// 検分規則1の執行: 有効な JSON でも ExportTraceServiceRequest の形に合わない行は
/// Err で返し、蔵ごと「通らない」へ倒す(形の壊れた行を不可視にしない)。
/// 属性値は正規化文字列(string はそのまま/int・double は十進表記/bool は true・false)。
/// 非スカラー(array/kvlist/bytes)は None で残し、突き合わせ時に「通らない」へ倒す。
fn collect_spans(
    v: &serde_json::Value,
    name: &str,
    out: &mut Vec<Vec<(String, Option<String>)>>,
) -> Result<(), String> {
    // 欄が無いのは可(空の ExportTraceServiceRequest は正当)。在るのに型が違うのは不可。
    let arr = |v: &serde_json::Value, key: &str| -> Result<Vec<serde_json::Value>, String> {
        match v.get(key) {
            None => Ok(Vec::new()),
            Some(x) => x
                .as_array()
                .cloned()
                .ok_or_else(|| format!("{key} が配列ではない")),
        }
    };
    if !v.is_object() {
        return Err("行の最上位がオブジェクトではない".into());
    }
    for rs in arr(v, "resourceSpans")? {
        for ss in arr(&rs, "scopeSpans")? {
            for span in arr(&ss, "spans")? {
                if !span.is_object() {
                    return Err("spans の要素がオブジェクトではない".into());
                }
                if span.get("name").and_then(|n| n.as_str()) != Some(name) {
                    continue;
                }
                let attrs = arr(&span, "attributes")?
                    .iter()
                    .filter_map(|a| {
                        let key = a.get("key")?.as_str()?.to_string();
                        Some((key, a.get("value").and_then(canonical)))
                    })
                    .collect();
                out.push(attrs);
            }
        }
    }
    Ok(())
}

/// OTLP/JSON の AnyValue をスカラーの正規化文字列へ。intValue は仕様上 JSON 文字列で
/// 来る(int64)が、数値で来る書き手にも寛容に応じる。
fn canonical(v: &serde_json::Value) -> Option<String> {
    if let Some(s) = v.get("stringValue").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    for key in ["intValue", "doubleValue"] {
        if let Some(x) = v.get(key) {
            return match x {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            };
        }
    }
    if let Some(b) = v.get("boolValue").and_then(|x| x.as_bool()) {
        return Some(b.to_string());
    }
    None
}
