//! 行数予算。肥大は banto の前身(bgit)の死因だった — 門も同じ掟に従う。
//! 上限を上げる変更は受けない。何かを足すなら、何を足さないかを先に。

use std::path::Path;

const BUDGET: usize = 3000;

fn count_dir(dir: &Path, total: &mut usize) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            count_dir(&path, total);
        } else if path.extension().is_some_and(|e| e == "rs") {
            *total += std::fs::read_to_string(&path).unwrap().lines().count();
        }
    }
}

#[test]
fn src_stays_within_budget() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut total = 0;
    for name in ["monban-core", "monban-cli"] {
        count_dir(&crates.join(name).join("src"), &mut total);
    }
    assert!(
        total <= BUDGET,
        "crates/*/src の合計 {total} 行が上限 {BUDGET} 行を超えた"
    );
}
