# go-html-template

Go の `html/template` に近い使い方を Rust で提供するクレートです。

## Features

- `Template::new(...).parse(...).execute(...)` の API
- `{{.}}`, `{{.Field}}`, `{{$}}`, `{{$.Field}}`
- `{{if}}`, `{{else}}`, `{{else if}}`, `{{end}}`
- `{{range}} ... {{else}} ... {{end}}`
- `{{with}} ... {{else}} ... {{end}}`
- `{{define "name"}} ... {{end}}` と `{{template "name" .}}`
- パイプライン (`{{.Name | upper}}`)
- HTML 自動エスケープ（`& < > " '`）
- `safe_html` / `html` / `len` / `not` / `eq` / `ne` / `and` / `or` / `print`
- `parse_files` / `parse_glob`

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

## Status

Go `html/template` の全機能（文脈依存エスケープ、メソッド解決、変数束縛、関数群の完全互換など）を完全再現したものではなく、
「同等の使い方」で実用しやすい主要機能を Rust で提供する実装です。
