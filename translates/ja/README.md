# go-html-template

Go の `html/template` に近い API と挙動を Rust で提供するクレートです。

Developed by [Finite Field, K.K.](https://finitefield.org)

英語版: [README (English)](../../README.md)

## Features

- API スタイル: `Template::new(...).parse(...).execute(...)`
- `delims`、`clone_template`、`option(missingkey=...)`
- データ参照: `{{.}}`, `{{.Field}}`, `{{$}}`, `{{$.Field}}`
- 変数: `{{$x := ...}}`, `{{$x = ...}}`, `{{$x}}`, `{{$x.Field}}`
- 制御構文: `{{if}}`, `{{else}}`, `{{else if}}`, `{{end}}`
- `range` ループ: `{{range}} ... {{else}} ... {{end}}`, `{{range $i, $v := ...}}`, `{{range $i, $v = ...}}`
- `with` ブロック: `{{with}} ... {{else}} ... {{end}}`, `{{else with ...}}`
- テンプレート定義と呼び出し: `{{define "name"}} ... {{end}}`, `{{template "name" .}}`
- ブロック: `{{block "name" .}} default {{end}}`
- `range` 内での `{{break}}` / `{{continue}}`
- パイプライン（例: `{{.Name | upper}}`）
- メソッド解決（`Template::add_method`, `{{.Obj.Method}}`, `{{.Obj.Method "arg"}}`）
- HTML本文・属性値・URL属性・`<script>`・`<style>` に対する文脈依存エスケープ
- parse 時のコンテキスト解析（分岐文脈の整合性、テンプレート呼び出し文脈、終端文脈チェック）
- URL 属性で危険なスキーム（例: `javascript:`）をブロック
- namespaced / `data-` / `xmlns:*` 属性の Go 互換寄り挙動
- テンプレートソース中の HTML コメント（`<!-- ... -->`）を除去
- 組み込み関数: `safe_html`, `html`, `js`, `urlquery`, `len`, `index`, `slice`, `not`, `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `and`, `or`, `print`, `printf`, `println`, `call`
- safe ラッパー生成: `safe_html`, `safe_html_attr`, `safe_js`, `safe_css`, `safe_url`, `safe_srcset`
- Go 互換ラッパー型: `HTML`, `HTMLAttr`, `JS`, `JSStr`, `CSS`, `URL`, `Srcset`（`Value` に変換可能）
- Missing key モード: `missingkey=default|invalid|zero|error`
- ファイル読み込みヘルパー: `parse_files`, `parse_glob`, `parse_fs`（メソッドとトップレベルの両方）
- `web-rust` では `parse_files` / `parse_glob` / `parse_fs` が即座に `TemplateError::Parse` を返す
- 初回実行後の `parse*` / `clone_template` を禁止（Go 互換寄りの安全制約）
- トップレベルヘルパー: `must`, `parse_files`, `parse_glob`, `parse_fs`
- エスケープヘルパー: `HTMLEscape*`, `JSEscape*`, `URLQueryEscaper`, `IsTrue`

## Usage

```rust
use go_html_template::{FuncMap, Template, Value};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut funcs = FuncMap::new();
    funcs.insert(
        "upper".to_string(),
        Arc::new(|args: &[Value]| {
            let v = args.first().map(|x| x.to_plain_string()).unwrap_or_default();
            Ok(Value::from(v.to_uppercase()))
        }),
    );

    let tpl = Template::new("page")
        .funcs(funcs)
        .parse(
            r#"
{{define "item"}}<li>{{.}}</li>{{end}}
<h1>{{.Title | upper}}</h1>
<ul>{{range .Items}}{{template "item" .}}{{else}}<li>empty</li>{{end}}</ul>
<p>{{.Raw | safe_html}}</p>
"#,
        )?;

    let out = tpl.execute_to_string(&serde_json::json!({
        "Title": "shopping",
        "Items": ["apple", "<orange>"],
        "Raw": "<em>trusted</em>"
    }))?;

    println!("{out}");
    Ok(())
}
```

`web-rust` を使う場合は、依存に feature を指定します。

```toml
[dependencies]
go_html_template = { version = "0.2.1", features = ["web-rust"] }
```

## `web-rust` feature

`web-rust` を有効化すると、`std` のファイル I/O を使えない環境向けに、ファイル読み込み系ヘルパーが無効化されます。

### 挙動

- 無効化される API（実行時に `TemplateError::Parse` を返す）:
  - `Template::parse_files`
  - `Template::parse_glob`
  - `Template::parse_fs`
  - `parse_files`
  - `parse_glob`
  - `parse_fs`
- `Template::parse` と `execute*` は通常どおり使用可能です。
- `web-rust` では、テンプレートはメモリ上の文字列として扱う想定です。

```rust
let main_tpl = Template::new("page")
    .parse("<h1>{{.Title}}</h1>{{template \"item\" .}}")
    .unwrap();
let item_tpl = Template::new("item")
    .parse("<li>{{.}}</li>")
    .unwrap();
```

実運用では `include_str!` や埋め込みアセットから文字列を渡し、同一プロセス内で `parse` する形を想定しています。

### `std` 有効時との違い

- `parse_files` / `parse_glob` / `parse_fs` によるファイル探索は利用できません。
- それ以外の parse 時の構文チェックと実行時挙動は通常ビルドと同等です。

## Status

`html/template` の主要ワークフロー（テンプレート構文、関数パイプライン、文脈依存エスケープ、parse 時コンテキスト検証、危険 URL の遮断、safe 型、主要ヘルパー関数）は実装済みです。

ただし、Go `html/template` との厳密な 1:1 互換は現時点では目標外です。既知の差分は次のとおりです。

- Go の `Error` / `ErrorCode` 体系との完全互換（現在は Rust 向けの `TemplateError`）
- `parse.Tree` / `AddParseTree` など Go 固有 API の完全再現
- すべてのエッジケースにおけるエラーメッセージや挙動の完全一致
