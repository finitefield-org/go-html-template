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

## ContextState Full-Recompute Reduction (Item 2)

Optimization applied:
- Added byte-oriented HTML text transition in `ContextTracker::try_refresh_html_text_with_delta` so `EscapeMode::Html` no longer falls back to full `ContextState::from_rendered(&rendered)` whenever the chunk contains `<`.
- Added seed+delta recompute path for `AttrQuoted` / `AttrUnquoted` (`try_refresh_cached_state_seeded_delta`) to avoid scanning the entire accumulated `rendered`.
- Kept full fallback for `in_open_tag` and `AttrName` to preserve dynamic attribute-name correctness.

### Parse Breakdown Before/After

Command:

```bash
cargo run --release --quiet --bin perf_parse_breakdown
```

| benchmark | before (avg_us) | after (avg_us) | change |
|---|---:|---:|---:|
| parse_tree_expr_20k | 8292 | 8276 | -0.2% |
| parse_expr_20k | 9249 | 8982 | -2.9% |
| parse_tree_html_mix | 8266 | 8288 | +0.3% |
| parse_html_mix | 13511 | 13646 | +1.0% |

### Selected Go Compare Case (Parse)

| case | loops | before rust_parse_us | after rust_parse_us | change | output_match |
|---|---:|---:|---:|---:|---|
| static_200k | 20 | 3025 | 2407 | -20.4% | true |

## Runtime Tracker Dependency Reduction For Fixed Expr Modes (Item 3)

Optimization applied:
- Added `runtime_mode` to `Node::Expr` and decide at parse-time whether mode needs runtime re-resolution.
- In execute, fixed-mode expressions skip `tracker.mode()` inference.
- For fixed `AttrKind::Normal` expressions, tracker context update now skips appending escaped output text (context is unchanged by escaping), reducing hot-path tracker work.

### Before/After (`attr_20k`, same command pattern)

| case | loops | before rust_parse_us | after rust_parse_us | parse change | before rust_execute_us | after rust_execute_us | exec change | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| attr_20k | 10 | 25256 | 27091 | +7.3% | 20909 | 12877 | -38.4% | true |

Reference command:

```bash
cargo run --release --quiet --bin compare_go_html_template -- \
  --template benchmarks/go_compare_cases/attr_20k.tmpl \
  --data benchmarks/go_compare_cases/data_main.json \
  --loops 10
```

## Static/Range Specialized Path Reinforcement (Item 4)

Optimization applied:
- `range` static-body fast path now computes a `RepeatedTextPlan` and skips tracker updates only when one-iteration state is proven invariant (state/url/js/css/json scan state unchanged).
- `append_repeated_text` now has a tracker-skip branch that repeats directly into output without building tracker context.
- Added top-level execute fast path for action-free text-only templates (excluding script/style tag content), bypassing tracker/render loop entirely.

### Before/After (selected)

| case | loops | before rust_parse_us | after rust_parse_us | parse change | before rust_execute_us | after rust_execute_us | exec change | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| range_no_vars | 30 | 13 | 20 | +53.8% | 1488 | 209 | -86.0% | true |
| static_200k | 20 | 1929 | 2099 | +8.8% | 599 | 467 | -22.0% | true |

Reference commands:

```bash
cargo run --release --quiet --bin compare_go_html_template -- \
  --template benchmarks/go_compare_cases/range_no_vars.tmpl \
  --data benchmarks/go_compare_cases/data_main.json \
  --loops 30

cargo run --release --quiet --bin compare_go_html_template -- \
  --template benchmarks/go_compare_cases/static_200k.tmpl \
  --data benchmarks/go_compare_cases/data_main.json \
  --loops 20
```

## Parse Text-Plan Precompute Tightening (Item 5)

Optimization applied:
- Added `should_prepare_text_plan_for_script_style` and moved `prepare_text_plan_for_script_style` behind this guard in parse analysis.
- Tightened precompute trigger to:
  - always allow script/style tag contexts,
  - skip non-text contexts,
  - skip text chunks without `<`,
  - only precompute in HTML text when an actual opening `<script`/`<style` tag is present.
- Replaced broad marker scans (`<script`, `</script`, `<style`, `</style`) with a single pass opener check.

### Parse Breakdown Before/After

Command:

```bash
cargo run --release --quiet --bin perf_parse_breakdown
```

| benchmark | before (avg_us) | after (avg_us) | change |
|---|---:|---:|---:|
| parse_tree_expr_20k | 8313 | 8427 | +1.4% |
| parse_expr_20k | 9468 | 9131 | -3.6% |
| parse_tree_html_mix | 8349 | 8278 | -0.9% |
| parse_html_mix | 14138 | 14087 | -0.4% |

### Selected Go Compare Case (Parse)

| case | loops | before rust_parse_us | after rust_parse_us | change | output_match |
|---|---:|---:|---:|---:|---|
| static_200k | 20 | 1979 | 1626 | -17.8% | true |

## Value Root Reuse In Compare Execute Path (Latest Re-run)

Optimization applied:
- Added Value-based execute APIs and switched compare tool execute loop to reuse one prebuilt root `Value` (`execute_value_to_string`) instead of re-serializing JSON each iteration.
- This removes comparator-side overhead and exposes engine execution cost more directly.

Updated files:
- `/Users/kazuyoshitoshiya/Documents/GitHub/go-html-template/src/lib.rs`
- `/Users/kazuyoshitoshiya/Documents/GitHub/go-html-template/src/bin/compare_go_html_template.rs`

Reference command pattern:

```bash
cargo run --release --quiet --bin compare_go_html_template -- \
  --template benchmarks/go_compare_cases/<case>.tmpl \
  --data benchmarks/go_compare_cases/data_main.json \
  --loops <case-specific>
```

### Full Comparison Snapshot

| case | loops | rust_parse_us | go_parse_us | parse_ratio | rust_exec_us | go_exec_us | exec_ratio | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| static_200k | 20 | 2573 | 123 | 20.92 | 593 | 397 | 1.49 | true |
| expr_20k | 20 | 9567 | 9922 | 0.96 | 2707 | 12631 | 0.21 | true |
| deep_path_20k | 20 | 18028 | 23482 | 0.77 | 2710 | 19359 | 0.14 | true |
| func_print_20k | 20 | 14497 | 14266 | 1.02 | 4567 | 21023 | 0.22 | true |
| range_no_vars | 30 | 13 | 12 | 1.08 | 123 | 1009 | 0.12 | true |
| range_var_decl | 30 | 13 | 14 | 0.93 | 9508 | 23265 | 0.41 | true |
| range_var_assign | 30 | 15 | 19 | 0.79 | 9783 | 23764 | 0.41 | true |
| if_else_20k | 20 | 37303 | 32393 | 1.15 | 6120 | 14929 | 0.41 | true |
| template_call_range | 30 | 14 | 15 | 0.93 | 6140 | 13222 | 0.46 | true |
| attr_20k | 10 | 26997 | 12407 | 2.18 | 12760 | 13771 | 0.93 | true |
| url_20k | 10 | 29162 | 13762 | 2.12 | 24156 | 24443 | 0.99 | true |
| script_2 | 30 | 14 | 11 | 1.27 | 1 | 11 | 0.09 | true |
| script_100 | 10 | 150 | 60 | 2.50 | 57 | 87 | 0.66 | true |
| script_2k | 3 | 5237 | 2524 | 2.07 | 1815 | 3602 | 0.50 | true |
| style_100 | 10 | 144 | 61 | 2.36 | 150 | 105 | 1.43 | true |
| style_2k | 3 | 3835 | 1177 | 3.26 | 1576 | 3877 | 0.41 | true |

### Remaining Rust>Go Cases

- Parse slower: `static_200k`, `func_print_20k` (near tie), `if_else_20k`, `attr_20k`, `url_20k`, `script_100`, `script_2k`, `style_100`, `style_2k`.
- Execute slower: `static_200k`, `style_100` (small).

## Dynamic Attr Runtime-Mode Flag (Parse-time decision tightening)

Optimization applied:
- Removed `current_tag_value_context(&tracker.rendered)` scan from `should_resolve_expr_mode_at_runtime`.
- Added incremental tracking in `ContextTracker`:
  - `attr_name_dynamic_pending`
  - `attr_value_from_dynamic_attr`
- Runtime mode is now enabled only when the current attribute value originated from a dynamic attribute name action.
- Added regression tests to guarantee correctness:
  - `parse_marks_dynamic_attribute_value_expr_as_runtime_mode`
  - `parse_keeps_static_attribute_value_expr_fixed_mode`

### Selected before/after (same compare command pattern)

| case | loops | before rust_parse_us | after rust_parse_us | parse change | before rust_exec_us | after rust_exec_us | exec change | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| attr_20k | 10 | 26997 | 25968 | -3.8% | 12760 | 12131 | -4.9% | true |
| url_20k | 10 | 29162 | 28894 | -0.9% | 24156 | 24388 | +1.0% | true |

## Parse Context Full-Scan Reduction (Item 2 follow-up)

Optimization applied:
- Added parse-time text transition cache in `ParseContextAnalyzer` keyed by normalized tracker state + short text fragment.
- Reused cached context transitions for repeated text chunks to avoid repeated `append_text` scanning in large repeated templates (`attr_20k` / `url_20k` style patterns).
- Added no-op fast paths:
  - `strip_html_comments` now returns borrowed text when no `<!--` exists.
  - `parse_tree` now skips tokenize/parse-node pipeline for delimiter-free input and creates a single text node directly.
- `source_without_actions` now returns borrowed text when no actions exist (avoids extra copy in hazard validation).

### Parse Breakdown Before/After

Command:

```bash
cargo run --release --quiet --bin perf_parse_breakdown
```

| benchmark | before (avg_us) | after (avg_us) | change |
|---|---:|---:|---:|
| parse_tree_expr_20k | 8451 | 8124 | -3.9% |
| parse_expr_20k | 9089 | 8814 | -3.0% |
| parse_tree_html_mix | 8165 | 7474 | -8.5% |
| parse_html_mix | 13642 | 10475 | -23.2% |

### Selected Go Compare Before/After

| case | loops | before rust_parse_us | after rust_parse_us | parse change | before rust_execute_us | after rust_execute_us | exec change | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| static_200k | 20 | 2062 | 1101 | -46.6% | 503 | 586 | +16.5% | true |
| attr_20k | 10 | 25968 | 16315 | -37.2% | 12131 | 12823 | +5.7% | true |
| url_20k | 10 | 28894 | 21920 | -24.1% | 24388 | 25164 | +3.2% | true |

## Parse Full-Scan Reduction (extra pass for static/html-heavy)

Additional optimization applied:
- `validate_unquoted_attr_hazards` now returns early when source cannot contain unquoted-attr hazards (`<` or `=` not present).
- `infer_escape_mode_with_tag_context` now guards script/style/title/textarea scans with cheap marker checks (`<script`, `<style`, `<title`, `<textarea`), reducing expensive full scans on unrelated HTML.
- `reanalyze_contexts` now has a text-only template fast path that bypasses `ParseContextAnalyzer` and directly computes context/state while preserving JS/CSS/script/style validation behavior.

### Selected before/after (against previous section’s baseline)

| case | loops | before rust_parse_us | after rust_parse_us | parse change | before rust_exec_us | after rust_exec_us | exec change | output_match |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| static_200k | 20 | 1101 | 525 | -52.3% | 586 | 481 | -17.9% | true |
| attr_20k | 10 | 16315 | 16519 | +1.2% | 12823 | 12648 | -1.4% | true |
| url_20k | 10 | 21920 | 15184 | -30.7% | 25164 | 25428 | +1.0% | true |
