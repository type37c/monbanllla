//! 関の執行。門番が自分でツールチェーンを走らせる —
//! コンパイラは説得できない独立検分者であり、持ち込みの実行結果は数えない。

use crate::Result;
use std::path::Path;
use std::process::Command;

/// 出力の打ち切り上限(証跡は台帳の蔵に入るため無限には抱えない)。
const MAX_OUTPUT: usize = 100_000;

/// 1回のツールチェーン実行の証跡。transcript がそのまま証拠ファイルになる。
#[derive(Debug)]
pub struct ToolchainRun {
    pub cmd: Vec<String>,
    pub exit: Option<i32>,
    pub ok: bool,
    pub transcript: String,
}

pub fn run_toolchain(cmd: &[String], expect_exit: i32, cwd: &Path) -> Result<ToolchainRun> {
    let output = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(cwd)
        .output()?;
    let exit = output.status.code();
    let ok = exit == Some(expect_exit);
    let mut body = String::new();
    body.push_str(&String::from_utf8_lossy(&output.stdout));
    body.push_str(&String::from_utf8_lossy(&output.stderr));
    if body.len() > MAX_OUTPUT {
        let cut = body
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= MAX_OUTPUT)
            .last()
            .unwrap_or(0);
        body.truncate(cut);
        body.push_str("\n…(出力が長いため打ち切り)\n");
    }
    let transcript = format!(
        "$ {}\n(exit: {}, expect: {expect_exit}, cwd: {})\n---\n{body}",
        cmd.join(" "),
        exit.map_or("signal".into(), |c| c.to_string()),
        cwd.display(),
    );
    Ok(ToolchainRun {
        cmd: cmd.to_vec(),
        exit,
        ok,
        transcript,
    })
}
