# monban — 門番

> **状態: v0.3 — toolchain / OTel の二つの証拠タイプと、エージェント向け MCP の口が動きます。**
> 契約([docs/contract_format_v0.md](docs/contract_format_v0.md)・[docs/mcp_v0.md](docs/mcp_v0.md))を
> 先に凍結し、コードがそれに従っています。OTel 検分の規則は
> [docs/otel_evidence_v0.md](docs/otel_evidence_v0.md)。
> (リポジトリ名は monbanllla、製品名は monban — 姉妹店 bantollla / banto と同じ対応です)

**門番は、エージェントとの契約を機械が執行する門である。
契約に書かれた証拠なしには、いかなる完了主張も通らない。**

プロンプトや CLAUDE.md は願いであって、契約ではありません(強制力がないからです)。
エージェントの「テストが通りました」「完了しました」は、往々にして通っていないし、
できていません。monban は、願いを機械執行の契約に変え、主張と事実のあいだに門を立てます。

番頭([banto](https://github.com/type37c/bantollla))が帳場を守るなら、門番は門を守ります。
同じ「番」の字を分け合う、対の道具です。台帳(大福帳)は banto が持ち、
門は monban が立てる — **門は交換可能、台帳は一つ**、という役割分担です。

## 契約(守る不変量 — 三条)

このリポジトリに入るすべてのコードは、次の三条に仕えます。三条に反する変更は入りません。

**第一条 — 証拠なしの主張は通らない。**
要約・自己評価・会話ログは証拠になりません。機械検査可能な独立源の証拠のみが
主張を通します。

**第二条 — 宣言者は自分を改められない。**
declare した actor と同じ actor による verify は、型と実行時の両方で拒まれます。
自分の仕事に自分で判を押すことは、この門ではできません。

**第三条 — 主張・証拠・判定は追記のみの台帳に鎖で残る。**
行の編集・削除は存在しません。間違いは訂正イベントを追記して直します。
歴史は消さず、上書きもしません。

## 守らないこと(明示)

門番が**やらない**ことを、先に約束します。

- 仕事の**質**は判定しません。門を通ったことが保証するのは「契約どおりの証拠がある」
  ことだけです
- エージェントを賢くしません。能力競争の外に立ちます。エージェントの能力の成長は、
  すべてこの門の追い風です
- 権限制御は v0 では扱いません(契約履行の実績から権限を発行する仕組みは第二段です。
  完了ゲートの台帳が、その前提になります)

## 何が新しいか

完了主張を一級のオブジェクトにする三点セット —
**① actor 分離、② 機械検査可能な独立証拠、③ 追記台帳の鎖** — の機械執行です。

ガードレール(内容の検閲)でも、事前の権限制御でも、監視(記録のみ。警報は判断では
ありません)でも、CI の必須チェック(actor の概念がありません)でもありません。
どれか一つなら既にあります。三点セットを一つの門として執行する者が、いません。

## 語彙(江戸の層)

| 語 | 意味 |
|---|---|
| 門番 monban | ゲート本体。契約を機械執行する門 |
| 手形 tegata | 証拠つき通過証。verified された claim |
| 改め aratame | verify。宣言者以外の actor による検分 |

## 形

- **local-first。** サーバーも、アカウントも、Web API も要りません
- **[bantollla](https://github.com/type37c/bantollla) の banto-kernel をクレート依存**し、
  追記台帳の型と鎖検査を輸入します。台帳の契約は
  [banto 契約 v1](https://github.com/type37c/bantollla/blob/main/docs/contract_v1.md) に凍結済みです
- 凍結すべき API は三つ: **契約ファイル形式・台帳イベントスキーマ・MCP ツール名**。
  API = 契約が、文字どおり成立します

## 前身の実証

この門は思いつきではありません。bantollla の
[otel-gate-demo](https://github.com/type37c/bantollla/tree/main/examples/otel-gate-demo) /
[otel-gate-demo2](https://github.com/type37c/bantollla/tree/main/examples/otel-gate-demo2) に、
三点セットの実録が公開のまま残っています — 独立 Collector(証人席)、actor 分離
(作業体・境界の運用者・検証者)、そして**捏造された「できました」が門で fail した台帳そのもの**。
monban は、この実証を再現手順の束から、誰でも据えられる道具に変えるものです。

## 三つの口

1. **エージェント向け — MCP ツール `monban.declare`。** 主張と証拠参照の提出。唯一の入口です。
   `monban mcp` で stdio サーバーが立ちます。**改めに相当する MCP ツールは、将来の版でも
   存在させません** — エージェントに判を渡さないことが、第二条の執行です([docs/mcp_v0.md](docs/mcp_v0.md))
2. **証拠の搬入。** 門番自身がツールチェーンを実行します(cargo build / test 等 —
   コンパイラは説得できない独立検分者です)。加えて OTel span を読みます
   (span は証拠の器、独立コレクタは証人席です。v0.3 で実装 —
   蔵は Collector file exporter の OTLP/JSON 行、規則は [docs/otel_evidence_v0.md](docs/otel_evidence_v0.md))。
   検分は門番という別 actor の仕事なので、ここで第二条の actor 分離が自然に成立します
3. **人間向け — CLI。** 改めの結果の閲覧と、上書き裁可です

## 使い方(v0.3)

banto のワークスペース(`banto init` した場所)に、契約 `monban.toml` を置きます。
台帳は banto のものをそのまま使います — 台帳は一つ、門は交換可能。

```sh
git clone https://github.com/type37c/monbanllla
cargo install --path monbanllla/crates/monban-cli   # バイナリ名は monban

cd あなたの作業場   # banto init 済みの場所
cat > monban.toml <<'EOF'
schema = "monban/0"

[[seki]]
name = "tests-pass"
title = "テストが通っている"

  [[seki.evidence]]
  kind = "toolchain"
  cmd = ["cargo", "test", "--workspace"]
EOF

monban contract                 # 契約の検分(証拠のない関はここで拒まれる)
monban check                    # 予行(台帳には書かない)

# エージェント(または人)が関を名指しで宣言する。証拠なしでは通らない
monban declare tests-pass "テストが通りました" --evidence report.md --actor agent:claude

# 改めは門番自身の仕事。ツールチェーンを自分で走らせ、判定を台帳に刻む
monban verify <宣言のイベントID>
#  → 手形(pass)か、止まる(fail)か。証跡は蔵(ledger/objects/)に残る
```

30秒のデモそのもの: `--evidence` を付けずに declare すれば**その場で止まり**、
台帳には何も残りません。契約を後から通りやすいものに差し替えて verify しても、
宣言時の契約ハッシュとの不一致で **fail が記帳されます**。

エージェントには MCP の口を与えます(例: Claude Code の MCP 設定に
`monban mcp` を登録)。エージェントが使えるのは `monban.declare` ただ一つで、
改めの口はありません。

## 証拠タイプ

- 第1号: **ツールチェーンの終了コード+出力ハッシュ**(門番が自分で走らせたもののみ)— v0.1 で実装済み
- 第2号: **OTel span**(エージェントの実行過程の証言)— v0.3 で実装済み。
  独立コレクタの蔵にある span と、宣言時に写された成果物を門番が突き合わせます。
  「やったか」は otel、「動くか」は toolchain — 同じ関に AND で並べるのが正しい使い方です
  ([docs/otel_evidence_v0.md](docs/otel_evidence_v0.md))

## 30秒のデモ

エージェント「テストが通りました」→ 門番が手形を要求 → ツールチェーン実行の証拠なし
→ **止まる**。

自信を持った偽完了が止まる瞬間が、この道具の一点突破です。v0.1 の統合テストは
この止まる瞬間そのもの(証拠なし宣言の拒否・自己改めの拒否・契約差し替えの検出)を
実バイナリで検めています(`crates/monban-cli/tests/cli.rs`)。

## 状態

v0 設計凍結・v0.2 実装・v0.3 で OTel 証拠(2026-07-31)。コードより先に契約を立て、コードがそれに従いました。
設計への異議・質問は Issues へどうぞ。三条そのものへの反対も、理由が添えてあれば歓迎します。

## English

**monban** ("gatekeeper") is the sister tool of
[banto](https://github.com/type37c/bantollla): a gate that machine-enforces
contracts with AI agents. No completion claim passes without the evidence the
contract names. Three invariants: evidence or no pass; the declaring actor can
never verify itself; claims, evidence, and verdicts live in an append-only,
hash-chained ledger. v0.3 ships two evidence kinds — toolchain (the gatekeeper
runs your test suite itself — compilers can't be persuaded) and OTel spans (the
gatekeeper reads an independent collector's vault and cross-checks span
attributes against the artifacts copied at declare time) — plus an MCP entry
point for agents (`monban mcp`; the single tool `monban.declare` — there is
deliberately no verify tool for agents, ever). Built on the `banto-kernel`
crate. Japanese-first; issues in English are welcome.

## License

Apache-2.0。関連技術は特許出願済みで、特許ライセンスの範囲は Apache-2.0 §3 の定めによります。
実装コードは banto と同じく Apache-2.0 で公開する方針で、公開されたコードには
§3 の特許許諾が伴います。隠すより先に書いておきます
(bantollla の「出自について、正直に」も参照)。
