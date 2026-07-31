//! monban-core — エージェントとの契約を機械が執行する門の芯。
//!
//! 三条(README.md)と契約ファイル形式(docs/contract_format_v0.md)が権威。
//! このクレートはその実装にすぎない。台帳は banto-kernel を輸入し、自前で発明しない。

pub mod contract;
pub mod gate;
pub mod otel;

pub use contract::{Contract, EvidenceReq, Seki, SCHEMA};
pub use gate::{run_toolchain, ToolchainRun};
pub use otel::{examine_otel, OtelRun};

/// 門番が台帳に書くときの actor 名。宣言(declare)には決して使えない —
/// 門番は改める側であり、自分の仕事に自分で判を押す構図を型で塞ぐ。
pub const GATE_ACTOR: &str = "agent:monban";

#[derive(Debug, thiserror::Error)]
pub enum MonbanError {
    #[error("契約を読めない: {0}")]
    Io(#[from] std::io::Error),
    #[error("契約の解析に失敗: {0}")]
    Parse(String),
    #[error("契約違反: {0}")]
    Invalid(String),
    #[error("関が見つからない: {0}")]
    SekiNotFound(String),
}

pub type Result<T> = std::result::Result<T, MonbanError>;
