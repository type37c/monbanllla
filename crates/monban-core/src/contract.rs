//! 契約ファイル `monban.toml` の読み込みと検証。
//! 形式の権威は docs/contract_format_v0.md。門は契約を読むだけで、要件を発明しない。

use crate::{MonbanError, Result};
use std::path::Path;

pub const SCHEMA: &str = "monban/0";

/// 読み込み済みの契約。`sha256` は契約ファイルそのもののハッシュで、
/// declare / verify のイベントに結ばれる(執行規則2: 契約の差し替えは台帳に写る)。
#[derive(Debug, Clone)]
pub struct Contract {
    pub seki: Vec<Seki>,
    pub sha256: String,
}

/// 関 — 契約上の通過点。claim は関を名指しで通る。
#[derive(Debug, Clone)]
pub struct Seki {
    pub name: String,
    pub title: String,
    pub evidence: Vec<EvidenceReq>,
}

/// 関を通るのに必要な証拠の要件。複数あれば全部(AND)。
#[derive(Debug, Clone)]
pub enum EvidenceReq {
    /// 門番が自分で走らせるツールチェーン。持ち込みの実行結果は数えない。
    Toolchain { cmd: Vec<String>, expect_exit: i32 },
}

impl Contract {
    pub fn load(path: &Path) -> Result<Contract> {
        let raw = std::fs::read_to_string(path)?;
        let sha256 = banto_kernel::hash_bytes(raw.as_bytes());
        let table = raw
            .parse::<toml::Table>()
            .map_err(|e| MonbanError::Parse(e.to_string()))?;

        let schema = table.get("schema").and_then(|v| v.as_str()).unwrap_or("");
        if schema != SCHEMA {
            return Err(MonbanError::Invalid(format!(
                "schema は \"{SCHEMA}\" でなければならない(実際: \"{schema}\")"
            )));
        }

        let seki_arr = table
            .get("seki")
            .and_then(|v| v.as_array())
            .ok_or_else(|| MonbanError::Invalid("[[seki]] が1つも無い".into()))?;
        let mut seki = Vec::new();
        for (i, s) in seki_arr.iter().enumerate() {
            seki.push(parse_seki(s, i)?);
        }
        if seki.is_empty() {
            return Err(MonbanError::Invalid("[[seki]] が1つも無い".into()));
        }
        for (i, a) in seki.iter().enumerate() {
            if seki[..i].iter().any(|b| b.name == a.name) {
                return Err(MonbanError::Invalid(format!("関の名前が重複: {}", a.name)));
            }
        }
        Ok(Contract { seki, sha256 })
    }

    pub fn find_seki(&self, name: &str) -> Result<&Seki> {
        self.seki
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| MonbanError::SekiNotFound(name.into()))
    }
}

fn parse_seki(v: &toml::Value, index: usize) -> Result<Seki> {
    let t = v
        .as_table()
        .ok_or_else(|| MonbanError::Invalid(format!("seki[{index}] がテーブルではない")))?;
    let name = t
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(MonbanError::Invalid(format!(
            "seki[{index}] の name は小文字ケバブケース必須(実際: \"{name}\")"
        )));
    }
    let title = t
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if title.is_empty() {
        return Err(MonbanError::Invalid(format!("関 {name}: title が無い")));
    }
    let ev_arr = t.get("evidence").and_then(|v| v.as_array());
    let mut evidence = Vec::new();
    if let Some(arr) = ev_arr {
        for e in arr {
            evidence.push(parse_evidence(e, &name)?);
        }
    }
    if evidence.is_empty() {
        return Err(MonbanError::Invalid(format!(
            "関 {name}: 証拠要件が1つも無い。証拠のない関は門ではない(執行規則1)"
        )));
    }
    Ok(Seki {
        name,
        title,
        evidence,
    })
}

fn parse_evidence(v: &toml::Value, seki: &str) -> Result<EvidenceReq> {
    let t = v
        .as_table()
        .ok_or_else(|| MonbanError::Invalid(format!("関 {seki}: evidence がテーブルではない")))?;
    let kind = t.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "toolchain" => {
            let cmd: Vec<String> = t
                .get("cmd")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if cmd.is_empty() {
                return Err(MonbanError::Invalid(format!(
                    "関 {seki}: toolchain の cmd は固定 argv 配列必須(シェル文字列は受けない)"
                )));
            }
            let expect_exit = t
                .get("expect_exit")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as i32;
            Ok(EvidenceReq::Toolchain { cmd, expect_exit })
        }
        "otel" => Err(MonbanError::Invalid(format!(
            "関 {seki}: 証拠タイプ otel は v0.1 では未実装(契約形式 v0 では予約)"
        ))),
        other => Err(MonbanError::Invalid(format!(
            "関 {seki}: 未知の証拠タイプ \"{other}\""
        ))),
    }
}
