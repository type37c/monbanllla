//! monban — エージェントとの契約を機械が執行する門。
//!
//! 三条: 証拠なしの主張は通らない/宣言者は自分を改められない/追記台帳の鎖。
//! 台帳は banto のワークスペース(banto.toml)のものを使う — 台帳は一つ、門は交換可能。

use clap::{Parser, Subcommand};
use monban_core::{Contract, EvidenceReq, GATE_ACTOR};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod mcp;

type R<T> = Result<T, String>;

#[derive(Parser)]
#[command(
    name = "monban",
    version,
    about = "門番 — エージェントとの契約を機械が執行する門"
)]
struct Cli {
    /// ワークスペースのルート(省略時は cwd から上へ banto.toml を探す)
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 契約(monban.toml)を読んで関の一覧を出す(検証を兼ねる)
    Contract,
    /// 関の検査を予行する(台帳には書かない)
    Check {
        /// 関の名前(省略時は全関)
        seki: Option<String>,
    },
    /// 完了を主張する(claim.declare。証拠必須・門番の名では宣言できない)
    Declare {
        /// 通ろうとする関の名前
        seki: String,
        /// 主張の一文
        title: String,
        /// 成果物そのもの(必須、複数可)。蔵(objects/)に写しが残る
        #[arg(long, required = true)]
        evidence: Vec<PathBuf>,
        /// 書き手(既定は banto.toml の actor、無ければ $USER)
        #[arg(long)]
        actor: Option<String>,
    },
    /// 改め(aratame): 門番自身が契約の検査を走らせ、claim.verify を記帳する
    Verify {
        /// 対象の claim.declare のイベント ID(64桁)
        claim_id: String,
    },
    /// MCP の口を stdio で立てる(ツールは monban.declare ただ一つ。docs/mcp_v0.md)
    Mcp,
}

struct Ws {
    root: PathBuf,
    actor: String,
    ledger_rel: String,
    objects_rel: String,
}

impl Ws {
    fn ledger(&self) -> banto_kernel::Result<banto_kernel::Ledger> {
        banto_kernel::Ledger::open(&self.root.join(&self.ledger_rel))
    }
}

/// ワークスペース発見。banto.toml の探し方・設定キーは banto CLI と同じ作法。
fn discover(root_flag: Option<&Path>, actor_flag: Option<&str>) -> R<Ws> {
    let root = match root_flag {
        Some(r) => std::fs::canonicalize(r).map_err(|e| format!("--root を解決できない: {e}"))?,
        None => {
            let mut dir =
                std::env::current_dir().map_err(|e| format!("cwd を取得できない: {e}"))?;
            loop {
                if dir.join("banto.toml").is_file() {
                    break dir;
                }
                if !dir.pop() {
                    return Err(
                        "banto.toml が見つからない。門は banto のワークスペースに立つ(`banto init`)"
                            .into(),
                    );
                }
            }
        }
    };
    if !root.join("banto.toml").is_file() {
        return Err(format!("banto.toml が見つからない: {}", root.display()));
    }
    let raw = std::fs::read_to_string(root.join("banto.toml"))
        .map_err(|e| format!("banto.toml を読めない: {e}"))?;
    let table: toml::Table = raw
        .parse()
        .map_err(|e| format!("banto.toml の解析に失敗: {e}"))?;
    let paths = table.get("paths").and_then(|v| v.as_table());
    let path_of = |key: &str, default: &str| -> String {
        paths
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    };
    let actor = actor_flag
        .map(str::to_owned)
        .or_else(|| {
            table
                .get("actor")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .or_else(|| std::env::var("USER").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "unknown".into());
    Ok(Ws {
        ledger_rel: path_of("ledger", "ledger/ledger.jsonl"),
        objects_rel: path_of("objects", "ledger/objects"),
        root,
        actor,
    })
}

fn load_contract(root: &Path) -> R<Contract> {
    let path = root.join("monban.toml");
    if !path.is_file() {
        return Err(format!(
            "monban.toml が見つからない: {}(契約より先にコードは動かない)",
            root.display()
        ));
    }
    Contract::load(&path).map_err(|e| e.to_string())
}

/// バイト列を蔵(objects/<sha256>)へ納め、証拠として返す。uri は蔵の中を指す
/// (banto の --store と同じ作法 — `banto verify --deep` がそのまま検められる)。
fn store_bytes(ws: &Ws, data: &[u8], media_type: &str) -> R<banto_kernel::Evidence> {
    let sha256 = banto_kernel::hash_bytes(data);
    let objects = ws.root.join(&ws.objects_rel);
    std::fs::create_dir_all(&objects).map_err(|e| format!("objects/ を作れない: {e}"))?;
    let dest = objects.join(&sha256);
    if !dest.is_file() {
        std::fs::write(&dest, data).map_err(|e| format!("objects/ へ書けない: {e}"))?;
    }
    Ok(banto_kernel::Evidence {
        uri: Some(format!(
            "{}/{}",
            ws.objects_rel.trim_end_matches('/'),
            sha256
        )),
        sha256,
        media_type: Some(media_type.into()),
        bytes: Some(data.len() as u64),
    })
}

fn store_file(ws: &Ws, file: &Path) -> R<banto_kernel::Evidence> {
    if !file.is_file() {
        return Err(format!("証拠ファイルが見つからない: {}", file.display()));
    }
    let data = std::fs::read(file).map_err(|e| format!("証拠ファイルを読めない: {e}"))?;
    store_bytes(ws, &data, "text/plain")
}

fn cmd_contract(ws: &Ws) -> R<i32> {
    let contract = load_contract(&ws.root)?;
    println!("契約 monban.toml(sha256 {})", &contract.sha256[..12]);
    for s in &contract.seki {
        println!(
            "関 {} — {}(証拠要件 {} 件)",
            s.name,
            s.title,
            s.evidence.len()
        );
        for e in &s.evidence {
            match e {
                EvidenceReq::Toolchain { cmd, expect_exit } => {
                    println!("  toolchain: {} (expect exit {expect_exit})", cmd.join(" "));
                }
                EvidenceReq::Otel {
                    vault,
                    span,
                    attrs_match,
                } => {
                    println!(
                        "  otel: span \"{span}\" in {vault} (attrs_match: {})",
                        if attrs_match.is_empty() {
                            "なし — 在否のみ".into()
                        } else {
                            attrs_match.join(", ")
                        }
                    );
                }
            }
        }
    }
    Ok(0)
}

/// 1件の証拠要件の検分結果。種類によらず「通ったか・証跡・台帳に書く形」を揃える。
enum Run {
    Tool(monban_core::ToolchainRun),
    Otel(monban_core::OtelRun),
}

impl Run {
    fn ok(&self) -> bool {
        match self {
            Run::Tool(r) => r.ok,
            Run::Otel(r) => r.ok,
        }
    }
    fn transcript(&self) -> &str {
        match self {
            Run::Tool(r) => &r.transcript,
            Run::Otel(r) => &r.transcript,
        }
    }
    fn check_json(&self) -> serde_json::Value {
        match self {
            Run::Tool(r) => serde_json::json!({
                "cmd": r.cmd.join(" "),
                "exit": r.exit,
                "ok": r.ok,
            }),
            Run::Otel(r) => serde_json::json!({
                "kind": "otel",
                "vault": r.vault,
                "span": r.span,
                "spans_found": r.spans_found,
                "ok": r.ok,
            }),
        }
    }
    fn label(&self) -> String {
        match self {
            Run::Tool(r) => format!(
                "{} (exit {})",
                r.cmd.join(" "),
                r.exit.map_or("signal".into(), |c| c.to_string())
            ),
            Run::Otel(r) => format!("otel span \"{}\"(蔵 {})", r.span, r.vault),
        }
    }
}

/// 関の全証拠要件を検分する。`artifacts` は宣言時に蔵へ写された成果物の(ラベル, 本文)—
/// otel の attrs_match が使う。None は予行(check)。
fn run_seki(
    contract: &Contract,
    name: &str,
    root: &Path,
    artifacts: Option<&[(String, String)]>,
) -> R<Vec<Run>> {
    let seki = contract.find_seki(name).map_err(|e| e.to_string())?;
    let mut runs = Vec::new();
    for req in &seki.evidence {
        match req {
            EvidenceReq::Toolchain { cmd, expect_exit } => {
                let run = monban_core::run_toolchain(cmd, *expect_exit, root)
                    .map_err(|e| format!("実行できない: {}: {e}", cmd.join(" ")))?;
                runs.push(Run::Tool(run));
            }
            EvidenceReq::Otel {
                vault,
                span,
                attrs_match,
            } => {
                let p = Path::new(vault);
                let path = if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    root.join(p)
                };
                runs.push(Run::Otel(monban_core::examine_otel(
                    &path,
                    span,
                    attrs_match,
                    artifacts,
                )));
            }
        }
    }
    Ok(runs)
}

fn cmd_check(ws: &Ws, seki: Option<&str>) -> R<i32> {
    let contract = load_contract(&ws.root)?;
    let names: Vec<String> = match seki {
        Some(n) => vec![n.to_string()],
        None => contract.seki.iter().map(|s| s.name.clone()).collect(),
    };
    let mut all_ok = true;
    for name in &names {
        for run in run_seki(&contract, name, &ws.root, None)? {
            let mark = if run.ok() { "✓" } else { "✗" };
            println!("{mark} 関 {name}: {}", run.label());
            all_ok &= run.ok();
        }
    }
    Ok(if all_ok { 0 } else { 1 })
}

/// 宣言の芯。CLI と MCP の口が共用する(規則は口によらず同じ — 三条一・三条二)。
fn declare_claim(
    ws: &Ws,
    seki: &str,
    title: &str,
    evidence: &[PathBuf],
    actor: String,
) -> R<banto_kernel::Envelope> {
    if actor == GATE_ACTOR {
        return Err(format!(
            "門番({GATE_ACTOR})は宣言しない。宣言と改めの actor は必ず別である(三条二)"
        ));
    }
    if evidence.is_empty() {
        return Err("宣言には成果物そのものの証拠が最低1件必要(三条一)".into());
    }
    let contract = load_contract(&ws.root)?;
    contract.find_seki(seki).map_err(|e| e.to_string())?;
    let mut ev = Vec::new();
    let mut artifacts = Vec::new();
    for f in evidence {
        ev.push(store_file(ws, f)?);
        artifacts.push(f.to_string_lossy().into_owned());
    }
    // 執行規則2: どの契約に向けた宣言かを台帳に結ぶ。契約原文の写しを蔵に納め、
    // ハッシュは body.contract に刻む(あとで契約が差し替わっても宣言時の原文が残る)。
    let contract_raw = std::fs::read(ws.root.join("monban.toml"))
        .map_err(|e| format!("monban.toml を読めない: {e}"))?;
    ev.push(store_bytes(ws, &contract_raw, "application/toml")?);
    let ledger = ws.ledger().map_err(|e| e.to_string())?;
    ledger
        .append(banto_kernel::Draft {
            ts: None,
            actor,
            event_type: "claim.declare".into(),
            body: serde_json::json!({
                "title": title,
                "seki": seki,
                "artifacts": artifacts,
                "contract": contract.sha256,
            }),
            evidence: ev,
            op: None,
        })
        .map_err(|e| e.to_string())
}

fn cmd_declare(
    ws: &Ws,
    seki: &str,
    title: &str,
    evidence: &[PathBuf],
    actor: Option<&str>,
) -> R<i32> {
    let actor = actor.map(str::to_owned).unwrap_or_else(|| ws.actor.clone());
    let event = declare_claim(ws, seki, title, evidence, actor)?;
    println!(
        "✓ 記録 seq={} type=claim.declare id={}",
        event.seq, event.id
    );
    println!("改めは門番の仕事: `monban verify {}`", event.id);
    Ok(0)
}

fn cmd_verify(ws: &Ws, claim_id: &str) -> R<i32> {
    let ledger = ws.ledger().map_err(|e| e.to_string())?;
    let claim = ledger
        .find(claim_id)
        .map_err(|_| format!("イベントが見つからない: {claim_id}"))?;
    if claim.event_type != "claim.declare" {
        return Err(format!(
            "{claim_id} は claim.declare ではない(type={})",
            claim.event_type
        ));
    }
    if claim.actor == GATE_ACTOR {
        return Err(format!(
            "この claim は門番自身({GATE_ACTOR})の宣言であり、門番は改められない(三条二)"
        ));
    }
    let seki_name = claim
        .body
        .get("seki")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{claim_id} の body に seki が無い(monban の関を通る宣言ではない)"))?
        .to_string();
    let contract = load_contract(&ws.root)?;

    // 執行規則2: 宣言時に結ばれた契約ハッシュと今の契約を突き合わせる。
    let declared_contract = claim.body.get("contract").and_then(|v| v.as_str());
    let contract_swapped = declared_contract.is_some_and(|h| h != contract.sha256);

    // otel の突き合わせ先は宣言時に蔵(objects/)へ写された成果物 — 作業ツリーは見ない。
    // 写しが無い・壊れているは誤りではなく「通らない」(証拠の消失を機械で捕まえる)。
    let seki_has_otel = contract.find_seki(&seki_name).is_ok_and(|s| {
        s.evidence
            .iter()
            .any(|e| matches!(e, EvidenceReq::Otel { .. }))
    });
    let artifacts_or_fail: Result<Vec<(String, String)>, String> = if seki_has_otel {
        let names: Vec<String> = claim
            .body
            .get("artifacts")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        names
            .iter()
            .zip(&claim.evidence)
            .map(|(name, e)| {
                let data = std::fs::read(ws.root.join(&ws.objects_rel).join(&e.sha256))
                    .map_err(|_| format!("宣言時の成果物の写しが蔵(objects/)に無い: {name}"))?;
                if banto_kernel::hash_bytes(&data) != e.sha256 {
                    return Err(format!("成果物の写しがハッシュと合わない: {name}"));
                }
                Ok((name.clone(), String::from_utf8_lossy(&data).into_owned()))
            })
            .collect()
    } else {
        Ok(Vec::new())
    };

    let (verdict, reason, runs) = if contract_swapped {
        (
            "fail",
            Some("契約ファイル(monban.toml)が宣言時から差し替わっている".to_string()),
            Vec::new(),
        )
    } else if let Err(lost) = &artifacts_or_fail {
        ("fail", Some(lost.clone()), Vec::new())
    } else {
        let artifacts = artifacts_or_fail.as_deref().unwrap_or(&[]);
        let runs = run_seki(&contract, &seki_name, &ws.root, Some(artifacts))?;
        let failed: Vec<String> = runs.iter().filter(|r| !r.ok()).map(Run::label).collect();
        if failed.is_empty() {
            ("pass", None, runs)
        } else {
            (
                "fail",
                Some(format!("検査失敗: {}", failed.join(" / "))),
                runs,
            )
        }
    };

    let mut ev = Vec::new();
    let mut checks = Vec::new();
    for run in &runs {
        ev.push(store_bytes(ws, run.transcript().as_bytes(), "text/plain")?);
        checks.push(run.check_json());
    }
    let mut body = serde_json::json!({
        "claim": claim.id,
        "verdict": verdict,
        "seki": seki_name,
        "contract": contract.sha256,
        "checks": checks,
    });
    if let Some(r) = &reason {
        body["reason"] = serde_json::json!(r);
    }
    let event = ledger
        .append(banto_kernel::Draft {
            ts: None,
            actor: GATE_ACTOR.into(),
            event_type: "claim.verify".into(),
            body,
            evidence: ev,
            op: None,
        })
        .map_err(|e| e.to_string())?;
    println!("✓ 記録 seq={} type=claim.verify id={}", event.seq, event.id);
    if verdict == "pass" {
        println!("手形 — 関 {seki_name} を通過(verified by {GATE_ACTOR})");
        Ok(0)
    } else {
        println!(
            "止まる — 関 {seki_name} は通れない: {}",
            reason.unwrap_or_default()
        );
        Ok(1)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = (|| -> R<i32> {
        match &cli.command {
            Cmd::Contract => cmd_contract(&discover(cli.root.as_deref(), None)?),
            Cmd::Check { seki } => {
                cmd_check(&discover(cli.root.as_deref(), None)?, seki.as_deref())
            }
            Cmd::Declare {
                seki,
                title,
                evidence,
                actor,
            } => {
                let ws = discover(cli.root.as_deref(), actor.as_deref())?;
                cmd_declare(&ws, seki, title, evidence, actor.as_deref())
            }
            Cmd::Verify { claim_id } => cmd_verify(&discover(cli.root.as_deref(), None)?, claim_id),
            Cmd::Mcp => mcp::serve(&discover(cli.root.as_deref(), None)?).map(|()| 0),
        }
    })();
    match result {
        Ok(code) => ExitCode::from(code as u8),
        Err(msg) => {
            eprintln!("monban: {msg}");
            ExitCode::from(1)
        }
    }
}
