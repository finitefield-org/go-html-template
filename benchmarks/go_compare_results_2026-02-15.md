# Go html/template vs go_html_template (Rust) Comparison Results

- Date: 2026-02-15
- Workspace: `/Users/kazuyoshitoshiya/Documents/GitHub/go-html-template`
- Comparator binary: `src/bin/compare_go_html_template.rs`
- Go runner: `tools/compare_go_html_template/main.go`
- Shared data: `benchmarks/go_compare_cases/data_main.json`

## How It Was Measured

Example command:

```bash
cargo run --release --quiet --bin compare_go_html_template -- \
  --template benchmarks/go_compare_cases/expr_20k.tmpl \
  --data benchmarks/go_compare_cases/data_main.json \
  --loops 20
```

- `parse_ratio = rust_parse_us / go_parse_us`
- `exec_ratio = rust_exec_us / go_exec_us`
- `output_match` is output equality between Rust and Go.

## Main Cases

| case | loops | rust_parse_us | go_parse_us | parse_ratio | rust_exec_us | go_exec_us | exec_ratio | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| static_200k | 20 | 2516 | 60 | 41.93 | 1471 | 302 | 4.87 | true |
| expr_20k | 20 | 32276 | 11030 | 2.93 | 13711 | 13285 | 1.03 | true |
| deep_path_20k | 20 | 47460 | 22740 | 2.09 | 13859 | 19729 | 0.70 | true |
| func_print_20k | 20 | 40625 | 13504 | 3.01 | 15857 | 21355 | 0.74 | true |
| range_no_vars | 30 | 16 | 6 | 2.67 | 17408 | 703 | 24.76 | true |
| range_var_decl | 30 | 18 | 6 | 3.00 | 58579 | 24071 | 2.43 | true |
| range_var_assign | 30 | 23 | 12 | 1.92 | 58440 | 24495 | 2.39 | true |
| if_else_20k | 20 | 92674 | 32523 | 2.85 | 15215 | 16266 | 0.94 | true |
| template_call_range | 30 | 20 | 11 | 1.82 | 30492 | 13739 | 2.22 | true |
| attr_20k | 10 | 80754 | 12582 | 6.42 | 41142 | 14801 | 2.78 | true |
| url_20k | 10 | 85494 | 13078 | 6.54 | 48874 | 25224 | 1.94 | true |

## Script/Style Stress Cases (Before Fix)

| case | loops | rust_parse_us | go_parse_us | parse_ratio | rust_exec_us | go_exec_us | exec_ratio | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| script_100 | 20 | 3284 | 57 | 57.61 | 1894 | 98 | 19.33 | false |
| style_100 | 20 | 2829 | 58 | 48.78 | 2515 | 136 | 18.49 | true |
| script_2k | 10 | 1161829 | 1116 | 1041.07 | 647473 | 1603 | 403.91 | false |
| style_2k | 10 | 999527 | 1221 | 818.61 | 900969 | 2297 | 392.24 | true |

## Script Output Mismatch (Repro, Before Fix)

Template: `benchmarks/go_compare_cases/script_2.tmpl`

- Rust preview:
  - `<script>const x="abc";</script><script>const x="abc";<//script>`
- Go preview:
  - `<script>const x="abc";</script><script>const x="abc";</script>`

This mismatch was observed consistently in `script_2`, `script_100`, and `script_2k`.

## Script Mismatch Fix Verification (After Fix)

After fixing duplicate `/` emission in script regexp filtering, the following checks now match Go:

| case | loops | rust_parse_us | go_parse_us | parse_ratio | rust_exec_us | go_exec_us | exec_ratio | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| script_2 | 30 | 22 | 5 | 4.40 | 84 | 5 | 16.80 | true |
| script_100 | 10 | 3317 | 64 | 51.83 | 2530 | 89 | 28.43 | true |
| script_2k | 3 | 1163774 | 1245 | 934.76 | 905910 | 2094 | 432.62 | true |

## Context Recompute Optimization (After Incremental Tracking)

Optimizations applied:
- `refresh_cached_state` moved to delta-based updates with full recompute fallback.
- `ContextState::from_rendered` now reuses precomputed tag-value context.
- `current_unclosed_tag_content` no longer allocates with `to_ascii_lowercase`.
- Script text filtering now accepts cached JS scan state to avoid rescanning full prefix.

### Before/After (selected)

| benchmark | before (avg_us) | after (avg_us) | change |
|---|---:|---:|---:|
| parse_expr_20k (`perf_parse_breakdown`) | 32014 | 19232 | -39.9% |
| parse_html_mix (`perf_parse_breakdown`) | 51930 | 31360 | -39.6% |
| expr_20k execute (Rust, compare tool) | 13711 | 3978 | -71.0% |
| range_no_vars execute (Rust, compare tool) | 17408 | 3871 | -77.8% |
| script_2k parse (Rust, compare tool) | 1163774 | 882644 | -24.0% |
| script_2k execute (Rust, compare tool) | 905910 | 563190 | -37.8% |
| style_2k parse (Rust, compare tool) | 999527 | 656331 | -34.3% |
| style_2k execute (Rust, compare tool) | 900969 | 551245 | -38.8% |

## Reanalyze Clone-Removal Optimization (Parse Phase)

Optimizations applied:
- `reanalyze_contexts` now analyzes under a single write lock without cloning the whole template map.
- `ParseContextAnalyzer` now works on `&mut HashMap<String, Vec<Node>>`.
- `analyze_template` now uses `remove/insert` per template to avoid `raw_nodes.clone()`.
- `analyze_nodes` keeps the single-flow fast path to skip unnecessary dedup.

### Before/After (`perf_parse_breakdown`)

| benchmark | before (avg_us) | after (avg_us) | change |
|---|---:|---:|---:|
| parse_tree_expr_20k | 8522 | 8425 | -1.1% |
| parse_expr_20k | 12166 | 10184 | -16.3% |
| parse_tree_html_mix | 8353 | 8446 | +1.1% |
| parse_html_mix | 20633 | 19012 | -7.9% |

## Range No-Vars Runtime Hotpath Optimization

Optimizations applied:
- `range` no-vars branch now reuses one scope (`clear()` each iteration) instead of push/pop per item.
- `Node::Text` in HTML mode skips `filter_html_text_sections` when text has no `<`.
- `filter_html_text_sections` now early-returns when no `<script`/`<style`, and removes `format!("{prefix}{output}")` rebuilds.

### Before/After (range_no_vars, compare tool)

| benchmark | loops | before rust_execute_us | after rust_execute_us | change | output_match |
|---|---:|---:|---:|---:|---|
| range_no_vars | 60 | 3871 | 3443 | -11.1% | true |

## URL/Attribute Escape Runtime Optimization

Optimizations applied:
- `escape_value_for_mode` now receives cached `url_part` from `ContextTracker` and avoids per-expression `url_part_context(rendered_prefix)` reparsing.
- URL attribute escaping now uses the cached `url_part` hint directly.
- HTML attribute escaping internals use byte-paths for ASCII (`append_html_attr_escaped_byte` fast path).

### Before/After (same command, loops=10)

| benchmark | before rust_execute_us | after rust_execute_us | change | output_match |
|---|---:|---:|---:|---|
| attr_20k | 16122 | 15567 | -3.4% | true |
| url_20k | 23790 | 20420 | -14.2% | true |

## Template/Data Files Saved

- Directory: `benchmarks/go_compare_cases`
- Files:
  - `data_main.json`
  - `static_200k.tmpl`
  - `expr_20k.tmpl`
  - `deep_path_20k.tmpl`
  - `func_print_20k.tmpl`
  - `range_no_vars.tmpl`
  - `range_var_decl.tmpl`
  - `range_var_assign.tmpl`
  - `if_else_20k.tmpl`
  - `template_call_range.tmpl`
  - `attr_20k.tmpl`
  - `url_20k.tmpl`
  - `script_2.tmpl`
  - `script_100.tmpl`
  - `script_2k.tmpl`
  - `style_100.tmpl`
  - `style_2k.tmpl`

## Re-run After Revert (2026-02-15)

Revert target:
- Linear/static template execution-plan experiment (branchless `Text + Expr` fast path).

Measurement command pattern:

```bash
cargo run --release --quiet --bin compare_go_html_template -- \
  --template benchmarks/go_compare_cases/<case>.tmpl \
  --data benchmarks/go_compare_cases/data_main.json \
  --loops <case-specific>
```

| case | loops | rust_parse_us | go_parse_us | parse_ratio | rust_exec_us | go_exec_us | exec_ratio | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| static_200k | 20 | 3025 | 58 | 52.16 | 1633 | 256 | 6.38 | true |
| expr_20k | 20 | 9520 | 9516 | 1.00 | 3073 | 13550 | 0.23 | true |
| deep_path_20k | 20 | 17830 | 22916 | 0.78 | 3108 | 20114 | 0.15 | true |
| func_print_20k | 20 | 14345 | 13602 | 1.05 | 4851 | 21742 | 0.22 | true |
| range_no_vars | 30 | 13 | 6 | 2.17 | 1488 | 651 | 2.29 | true |
| range_var_decl | 30 | 13 | 7 | 1.86 | 9225 | 24116 | 0.38 | true |
| range_var_assign | 30 | 16 | 9 | 1.78 | 9245 | 24535 | 0.38 | true |
| if_else_20k | 20 | 38904 | 32571 | 1.19 | 6921 | 16192 | 0.43 | true |
| template_call_range | 30 | 14 | 6 | 2.33 | 5258 | 13489 | 0.39 | true |
| attr_20k | 10 | 22800 | 12609 | 1.81 | 16386 | 15129 | 1.08 | true |
| url_20k | 10 | 26707 | 12588 | 2.12 | 21617 | 26055 | 0.83 | true |
| script_2 | 30 | 14 | 5 | 2.80 | 79 | 4 | 19.75 | true |
| script_100 | 10 | 1036 | 71 | 14.59 | 155 | 92 | 1.68 | true |
| script_2k | 3 | 330171 | 1285 | 256.94 | 1282 | 2291 | 0.56 | true |
| style_100 | 10 | 837 | 73 | 11.47 | 144 | 127 | 1.13 | true |
| style_2k | 3 | 252130 | 1271 | 198.37 | 1112 | 3279 | 0.34 | true |

## Action-Context Validation Cache Optimization (Item 1)

Optimization applied:
- `validate_action_context_before_insertion` now uses `ContextTracker` cached state (`js_scan_state`, `in_js_attribute`, `in_css_attribute`) instead of rebuilding JS/CSS prefixes and rescanning per action.
- Slash ambiguity check now reads JS expr context from tracker cache (`tracker_script_expr_context`) instead of rescanning rendered prefixes.
- Trailing escape check switched to suffix byte scan (`has_unfinished_escape_suffix`) to avoid full-prefix allocations/scans.

### Before/After (same templates, same loops)

| case | loops | before rust_parse_us | after rust_parse_us | parse change | before rust_exec_us | after rust_exec_us | exec change | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| script_100 | 10 | 1036 | 164 | -84.2% | 155 | 144 | -7.1% | true |
| style_100 | 10 | 837 | 207 | -75.3% | 144 | 141 | -2.1% | true |
| script_2k | 3 | 330171 | 2844 | -99.1% | 1282 | 1396 | +8.9% | true |
| style_2k | 3 | 252130 | 2984 | -98.8% | 1112 | 1209 | +8.7% | true |
