# go-html-template

A pure Rust crate that provides a Go `html/template`-like API and behavior.
No Go runtime, cgo, or FFI is required.

Developed by [Finite Field, K.K.](https://finitefield.org)

Japanese translation: [README (日本語)](translates/ja/README.md)

## Features

- API style: `Template::new(...).parse(...).execute(...)`
- `delims`, `clone_template`, and `option(missingkey=...)`
- Data access: `{{.}}`, `{{.Field}}`, `{{$}}`, `{{$.Field}}`
- Variables: `{{$x := ...}}`, `{{$x = ...}}`, `{{$x}}`, `{{$x.Field}}`
- Control flow: `{{if}}`, `{{else}}`, `{{else if}}`, `{{end}}`
- Range loops: `{{range}} ... {{else}} ... {{end}}`, `{{range $i, $v := ...}}`, `{{range $i, $v = ...}}`
- With blocks: `{{with}} ... {{else}} ... {{end}}`, `{{else with ...}}`
- Template definitions and calls: `{{define "name"}} ... {{end}}`, `{{template "name" .}}`
- Blocks: `{{block "name" .}} default {{end}}`
- `{{break}}` and `{{continue}}` inside `range`
- Pipelines (for example, `{{.Name | upper}}`)
- Method resolution (`Template::add_method`, `{{.Obj.Method}}`, `{{.Obj.Method "arg"}}`)
- Context-aware escaping for HTML text, attribute values, URL attributes, `<script>`, and `<style>`
- Parse-time context analysis (branch context consistency, template-call context checks, end-context checks)
- Blocking unsafe URL schemes in URL attributes (for example, `javascript:`)
- Go-compatible-ish handling for namespaced / `data-` / `xmlns:*` attributes
- Removal of HTML comments (`<!-- ... -->`) from template source
- Built-in funcs: `safe_html`, `html`, `js`, `urlquery`, `len`, `index`, `slice`, `not`, `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `and`, `or`, `print`, `printf`, `println`, `call`
- Safe wrapper constructors: `safe_html`, `safe_html_attr`, `safe_js`, `safe_css`, `safe_url`, `safe_srcset`
- Go-compatible wrapper types: `HTML`, `HTMLAttr`, `JS`, `JSStr`, `CSS`, `URL`, `Srcset` (convertible to `Value`)
- Missing key modes: `missingkey=default|invalid|zero|error`
- File helpers: `parse_files`, `parse_glob`, `parse_fs` (methods and top-level helpers)
- With `web-rust`, `parse_files` / `parse_glob` / `parse_fs` fail immediately with `TemplateError::Parse`
- `parse*` and `clone_template` are disallowed after first execution (Go-like safety constraint)
- Top-level helpers: `must`, `parse_files`, `parse_glob`, `parse_fs`
- Escape helpers: `HTMLEscape*`, `JSEscape*`, `URLQueryEscaper`, `IsTrue`

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

To use `web-rust`, enable the feature in your dependency:

```toml
[dependencies]
go_html_template = { version = "0.2.1", features = ["web-rust"] }
```

## `web-rust` feature

When `web-rust` is enabled, file I/O-based parsing helpers are disabled for environments where `std` file APIs are unavailable.

### Behavior

- Disabled APIs (return `TemplateError::Parse` at runtime):
  - `Template::parse_files`
  - `Template::parse_glob`
  - `Template::parse_fs`
  - `parse_files`
  - `parse_glob`
  - `parse_fs`
- `Template::parse` and `execute*` APIs continue to work normally.
- Expected usage in `web-rust`: load templates from in-memory strings.

```rust
let main_tpl = Template::new("page")
    .parse("<h1>{{.Title}}</h1>{{template \"item\" .}}")
    .unwrap();
let item_tpl = Template::new("item")
    .parse("<li>{{.}}</li>")
    .unwrap();
```

In practice, supply template strings via `include_str!` or embedded assets and call `parse` in-process.

### Difference from `std`-enabled file APIs

- File discovery with `parse_files` / `parse_glob` / `parse_fs` is not available.
- Parse-time syntax checks and runtime behavior are otherwise the same as in normal builds.

## Status

Core `html/template` workflows are implemented: template syntax, function pipelines, context-aware escaping, parse-time context checks, unsafe-URL blocking, safe types, and major helper functions.

This crate is not yet a strict 1:1 compatibility target for Go `html/template`. Known gaps include:

- Full compatibility with Go `Error` / `ErrorCode` taxonomy (currently Rust-oriented `TemplateError`)
- Full parity for Go-specific APIs such as `parse.Tree` / `AddParseTree`
- Exact match for every edge-case error message and behavior detail
