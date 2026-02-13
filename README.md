# go-html-template

Go の `html/template` に近い使い方を Rust で提供するクレートです。

## Features

- `Template::new(...).parse(...).execute(...)` の API
- `delims` / `clone_template` / `option(missingkey=...)`
- `{{.}}`, `{{.Field}}`, `{{$}}`, `{{$.Field}}`
- `{{$x := ...}}`, `{{$x = ...}}`, `{{$x}}`, `{{$x.Field}}`
- `{{if}}`, `{{else}}`, `{{else if}}`, `{{end}}`
- `{{range}} ... {{else}} ... {{end}}` / `{{range $i, $v := ...}}` / `{{range $i, $v = ...}}`
- `{{with}} ... {{else}} ... {{end}}` / `{{else with ...}}`
- `{{define "name"}} ... {{end}}` と `{{template "name" .}}`
- `{{block "name" .}} default {{end}}`
- `{{break}}` / `{{continue}}`（`range` 内）
- パイプライン (`{{.Name | upper}}`)
- メソッド解決（`Template::add_method` / `{{.Obj.Method}}` / `{{.Obj.Method "arg"}}`）
- 文脈依存エスケープ（HTML本文 / 属性値 / URL属性 / `<script>` / `<style>`）
- parse時コンテキスト解析（分岐文脈整合、テンプレート呼び出し文脈、終端文脈の静的検証）
- URL属性の危険スキーム（`javascript:` など）を遮断
- namespaced / `data-` / `xmlns:*` 属性の Go 互換寄り文脈判定
- HTML コメント（`<!-- ... -->`）のテンプレートソース除去
- `safe_html` / `html` / `js` / `urlquery` / `len` / `index` / `slice` / `not` / `eq` / `ne` / `lt` / `le` / `gt` / `ge` / `and` / `or` / `print` / `printf` / `println` / `call`
- safe 型: `safe_html` / `safe_html_attr` / `safe_js` / `safe_css` / `safe_url` / `safe_srcset`
- Go 互換型: `HTML` / `HTMLAttr` / `JS` / `JSStr` / `CSS` / `URL` / `Srcset`（`Value` へ変換可）
- `Option` 相当: `missingkey=default|invalid|zero|error`
- `parse_files` / `parse_glob` / `parse_fs`（method + top-level helper）
- `web-rust` feature を有効化した場合、`parse_files` / `parse_glob` / `parse_fs` は `Parse` エラーで即時失敗します。
- 実行後の `parse*` / `clone_template` を禁止（Go 互換寄り制約）
- top-level helper: `must`, `parse_files`, `parse_glob`, `parse_fs`
- escape helper: `HTMLEscape*`, `JSEscape*`, `URLQueryEscaper`, `IsTrue`

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

`web-rust` を使う場合は依存に feature を付けます。

```toml
[dependencies]
go_html_template = { version = "0.1.0", features = ["web-rust"] }
```

## `web-rust` feature

`web-rust` feature を有効化すると、`std` のファイル入出力 API が使えない環境向けに、ファイル読み込み系のヘルパーを無効化します。

### 動作方針

- 無効化されるAPI（実行時に `TemplateError::Parse`）
  - `Template::parse_files`
  - `Template::parse_glob`
  - `Template::parse_fs`
  - `parse_files`
  - `parse_glob`
  - `parse_fs`
- `Template::parse` と `execute*` 系は通常どおり利用できます。
- `web-rust` 側では、テンプレートは文字列を使って読み込む想定です。

```rust
let main_tpl = Template::new("page")
    .parse("<h1>{{.Title}}</h1>{{template \"item\" .}}")
    .unwrap();
let item_tpl = Template::new("item")
    .parse("<li>{{.}}</li>")
    .unwrap();
```

上位構造で `include_str!` や埋め込みデータから文字列を供給し、同一プロセス内で `parse` する形が実運用です。

### `std` のみ依存する既存 API との違い

- Go 実装と同様の `parse_files` / `parse_glob` / `parse_fs` によるファイル探索は使えません。
- `parse` 時点の文法・実行時挙動は通常ビルドと同等です。

## Status

主要な `html/template` ワークフロー（テンプレート構文、関数パイプライン、文脈依存エスケープ、parse時コンテキスト解析、危険URL遮断、safe型、主要ヘルパー関数）は実装済みです。

一方で、Go `html/template` との完全な1:1互換はまだ目標外です。特に以下は未完全です。

- Go の `Error` / `ErrorCode` 体系の完全互換（現状は Rust 向け `TemplateError`）
- `parse.Tree` / `AddParseTree` など、Go 固有 API の完全再現
- 仕様の細部（全エッジケースのエラーメッセージや挙動）の完全一致
