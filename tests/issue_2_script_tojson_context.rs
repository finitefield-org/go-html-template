use go_html_template::{Result as TemplateResult, Template, Value};
use serde_json::{json, Map, Value as JsonValue};
use std::fs;
use tempfile::tempdir;

fn to_json(args: &[Value]) -> TemplateResult<Value> {
    Ok(args.first().cloned().unwrap_or_else(|| Value::from(json!(null))))
}

#[test]
fn tojson_inside_large_executable_script_stays_in_js_context() {
    let template = Template::new("issue-2")
        .add_func("toJSON", to_json)
        .parse(
            r#"<script>
(() => {
  "use strict";

  const MICRO = 1000000n;
  const HUNDRED_PPM = 1000000n;

  const text = {
    resultCopied: {{toJSON .Page.Tool.Messages.ResultCopied}},
    shareCopied: {{toJSON .Page.Tool.Messages.ShareCopied}},
    typeLabels: {
      percent: {{toJSON .Page.Tool.Types.Percent}},
      fixed: {{toJSON .Page.Tool.Types.Fixed}}
    }
  };
})();
</script>"#,
        )
        .expect("parse template");

    let rendered = template
        .execute_to_string(&json!({
            "Page": {
                "Tool": {
                    "Messages": {
                        "ResultCopied": "結果をコピーしました。",
                        "ShareCopied": "共有リンクをコピーしました。"
                    },
                    "Types": {
                        "Percent": "%引き",
                        "Fixed": "固定額引き"
                    }
                }
            }
        }))
        .expect("render template");

    assert!(rendered.contains(r#"resultCopied: "結果をコピーしました。""#), "got: {rendered}");
    assert!(rendered.contains(r#"shareCopied: "共有リンクをコピーしました。""#), "got: {rendered}");
    assert!(rendered.contains(r#"percent: "%引き""#), "got: {rendered}");
    assert!(rendered.contains(r#"fixed: "固定額引き""#), "got: {rendered}");
    assert!(!rendered.contains("&#34;"), "got html escaping instead of JS escaping: {rendered}");
}

#[test]
fn tojson_inside_issue_2_repro_fragment_stays_quoted() {
    let message_fields = [
        ("ResultCopied", "結果をコピーしました。"),
        ("ShareCopied", "共有リンクをコピーしました。"),
        (
            "DiscountCapped",
            "値引き額が価格を超えたため、0円を下限として計算しました。",
        ),
        ("CompareFull", "比較シナリオは最大6件までです。"),
        ("StackFull", "併用ステップは最大5件までです。"),
        ("LoadScenario", "比較条件を入力欄へ読み込みました。"),
        ("ActiveTier", "現在の数量でこの段階が適用されます。"),
        (
            "BundleHigherThanList",
            "指定したセット価格が定価より高くなっています。条件を確認してください。",
        ),
    ];
    let validation_fields = [
        ("UnitPriceRequired", "元価格は0以上の数値で入力してください。"),
        ("Quantity", "数量は1以上の整数で入力してください。"),
        ("Rate", "割引率は0から100の範囲で入力してください。"),
        ("FixedAmount", "値引き額は0以上の数値で入力してください。"),
        ("BundleN", "セット個数は2以上の整数で入力してください。"),
        (
            "TierDuplicate",
            "数量条件が重複しています。段階割引の閾値は重ならないようにしてください。",
        ),
        ("TierThreshold", "段階割引の数量条件は1以上の整数で入力してください。"),
        ("CompareLimit", "比較シナリオは最大6件までです。"),
    ];
    let compare_fields = [
        ("Best", "最もお得"),
        ("TieBest", "同率で最安"),
        ("GapPrefix", "最安との差"),
        ("ScenarioLabel", "比較条件"),
        ("StackedLabel", "割引併用"),
    ];
    let action_fields = [
        ("LoadScenario", "入力へ反映"),
        ("DuplicateScenario", "複製"),
        ("DeleteScenario", "削除"),
        ("MoveUp", "上へ"),
        ("MoveDown", "下へ"),
        ("DeleteStep", "ステップを削除"),
    ];
    let type_fields = [
        ("Percent", "%引き"),
        ("Fixed", "固定額引き"),
        ("SecondHalf", "2点目半額"),
        ("Bundle", "n個で割引"),
        ("Tier", "まとめ買い"),
    ];

    let mut messages = Map::new();
    for (key, value) in message_fields {
        messages.insert(key.to_string(), json!(value));
    }

    let mut validation = Map::new();
    for (key, value) in validation_fields {
        validation.insert(key.to_string(), json!(value));
    }

    let mut compare = Map::new();
    for (key, value) in compare_fields {
        compare.insert(key.to_string(), json!(value));
    }

    let mut actions = Map::new();
    for (key, value) in action_fields {
        actions.insert(key.to_string(), json!(value));
    }

    let mut types = Map::new();
    for (key, value) in type_fields {
        types.insert(key.to_string(), json!(value));
    }

    let data = json!({
        "Page": {
            "Tool": {
                "Messages": JsonValue::Object(messages),
                "EmptyResult": "元価格を入力すると結果がここに表示されます。",
                "Validation": JsonValue::Object(validation),
                "Compare": JsonValue::Object(compare),
                "Actions": JsonValue::Object(actions),
                "Options": {
                    "RoundingNone": "なし"
                },
                "BrowserOnly": "この機能は現在のブラウザ内で動作します。",
                "Types": JsonValue::Object(types)
            }
        }
    });

    let template = Template::new("issue-2-full")
        .add_func("toJSON", to_json)
        .parse(
            r#"<script>
(() => {
  "use strict";

  const MICRO = 1000000n;
  const HUNDRED_PPM = 1000000n;

  const text = {
    resultCopied: {{toJSON .Page.Tool.Messages.ResultCopied}},
    shareCopied: {{toJSON .Page.Tool.Messages.ShareCopied}},
    discountCapped: {{toJSON .Page.Tool.Messages.DiscountCapped}},
    compareFull: {{toJSON .Page.Tool.Messages.CompareFull}},
    stackFull: {{toJSON .Page.Tool.Messages.StackFull}},
    loadScenario: {{toJSON .Page.Tool.Messages.LoadScenario}},
    emptyResult: {{toJSON .Page.Tool.EmptyResult}},
    invalidUnitPrice: {{toJSON .Page.Tool.Validation.UnitPriceRequired}},
    invalidQuantity: {{toJSON .Page.Tool.Validation.Quantity}},
    invalidRate: {{toJSON .Page.Tool.Validation.Rate}},
    invalidFixed: {{toJSON .Page.Tool.Validation.FixedAmount}},
    invalidBundleN: {{toJSON .Page.Tool.Validation.BundleN}},
    invalidTierDuplicate: {{toJSON .Page.Tool.Validation.TierDuplicate}},
    invalidTierThreshold: {{toJSON .Page.Tool.Validation.TierThreshold}},
    invalidCompare: {{toJSON .Page.Tool.Validation.CompareLimit}},
    bestDeal: {{toJSON .Page.Tool.Compare.Best}},
    tieBestDeal: {{toJSON .Page.Tool.Compare.TieBest}},
    gapPrefix: {{toJSON .Page.Tool.Compare.GapPrefix}},
    scenarioLabel: {{toJSON .Page.Tool.Compare.ScenarioLabel}},
    stackedLabel: {{toJSON .Page.Tool.Compare.StackedLabel}},
    loadAction: {{toJSON .Page.Tool.Actions.LoadScenario}},
    duplicateAction: {{toJSON .Page.Tool.Actions.DuplicateScenario}},
    deleteAction: {{toJSON .Page.Tool.Actions.DeleteScenario}},
    moveUpAction: {{toJSON .Page.Tool.Actions.MoveUp}},
    moveDownAction: {{toJSON .Page.Tool.Actions.MoveDown}},
    deleteStepAction: {{toJSON .Page.Tool.Actions.DeleteStep}},
    activeTierRow: {{toJSON .Page.Tool.Messages.ActiveTier}},
    noRounding: {{toJSON .Page.Tool.Options.RoundingNone}},
    currentBrowserOnly: {{toJSON .Page.Tool.BrowserOnly}},
    bundleHigherThanList: {{toJSON .Page.Tool.Messages.BundleHigherThanList}},
    typeLabels: {
      percent: {{toJSON .Page.Tool.Types.Percent}},
      fixed: {{toJSON .Page.Tool.Types.Fixed}},
      second_half: {{toJSON .Page.Tool.Types.SecondHalf}},
      bundle: {{toJSON .Page.Tool.Types.Bundle}},
      tier: {{toJSON .Page.Tool.Types.Tier}}
    }
  };
})();
</script>"#,
        )
        .expect("parse template");

    let rendered = template.execute_to_string(&data).expect("render template");

    let expected_snippets = [
        r#"resultCopied: "結果をコピーしました。""#,
        r#"shareCopied: "共有リンクをコピーしました。""#,
        r#"discountCapped: "値引き額が価格を超えたため、0円を下限として計算しました。""#,
        r#"invalidTierDuplicate: "数量条件が重複しています。段階割引の閾値は重ならないようにしてください。""#,
        r#"bundleHigherThanList: "指定したセット価格が定価より高くなっています。条件を確認してください。""#,
        r#"second_half: "2点目半額""#,
        r#"tier: "まとめ買い""#,
    ];

    for snippet in expected_snippets {
        assert!(rendered.contains(snippet), "missing snippet {snippet:?} in: {rendered}");
    }
    assert!(!rendered.contains("&#34;"), "got html escaping instead of JS escaping: {rendered}");
}

#[test]
fn tojson_inside_template_call_within_script_stays_quoted() {
    let template = Template::new("issue-2-template-call")
        .add_func("toJSON", to_json)
        .parse(
            r#"{{define "payload"}}
resultCopied: {{toJSON .Page.Tool.Messages.ResultCopied}},
shareCopied: {{toJSON .Page.Tool.Messages.ShareCopied}},
typeLabels: {
  percent: {{toJSON .Page.Tool.Types.Percent}},
  fixed: {{toJSON .Page.Tool.Types.Fixed}}
}
{{end}}
<script>
(() => {
  "use strict";
  const MICRO = 1000000n;
  const text = {
    {{template "payload" .}}
  };
})();
</script>"#,
        )
        .expect("parse template");

    let rendered = template
        .execute_to_string(&json!({
            "Page": {
                "Tool": {
                    "Messages": {
                        "ResultCopied": "結果をコピーしました。",
                        "ShareCopied": "共有リンクをコピーしました。"
                    },
                    "Types": {
                        "Percent": "%引き",
                        "Fixed": "固定額引き"
                    }
                }
            }
        }))
        .expect("render template");

    assert!(rendered.contains(r#"resultCopied: "結果をコピーしました。""#), "got: {rendered}");
    assert!(rendered.contains(r#"shareCopied: "共有リンクをコピーしました。""#), "got: {rendered}");
    assert!(rendered.contains(r#"percent: "%引き""#), "got: {rendered}");
    assert!(rendered.contains(r#"fixed: "固定額引き""#), "got: {rendered}");
    assert!(!rendered.contains("&#34;"), "got html escaping instead of JS escaping: {rendered}");
}

#[test]
fn parse_files_page_execution_keeps_script_json_quoting_with_title_in_other_template() {
    let dir = tempdir().expect("create temp dir");
    let base = dir.path().join("base.tmpl");
    let page = dir.path().join("page.tmpl");

    fs::write(
        &base,
        "{{define \"base\"}}<!doctype html><html><head><title>{{.Meta.Title}}</title></head><body>{{template \"page\" .}}</body></html>{{end}}",
    )
    .expect("write base");
    fs::write(
        &page,
        "{{define \"page\"}}<script>const text = { resultCopied: {{toJSON .Page.ResultCopied}} };</script>{{end}}",
    )
    .expect("write page");

    let template = Template::new("base.tmpl")
        .add_func("toJSON", to_json)
        .option("missingkey=error")
        .expect("set option")
        .parse_files([base.as_path(), page.as_path()])
        .expect("parse files");

    let rendered = template
        .execute_template_to_string(
            "page",
            &json!({
                "Meta": { "Title": "Sample" },
                "Page": { "ResultCopied": "Result copied." }
            }),
        )
        .expect("render page");

    assert_eq!(
        rendered,
        "<script>const text = { resultCopied: \"Result copied.\" };</script>"
    );

    let rendered_base = template
        .execute_template_to_string(
            "base",
            &json!({
                "Meta": { "Title": "Sample" },
                "Page": { "ResultCopied": "Result copied." }
            }),
        )
        .expect("render base");

    assert!(rendered_base.contains("<title>Sample</title>"), "got: {rendered_base}");
    assert!(
        rendered_base.contains(r#"resultCopied: "Result copied.""#),
        "got: {rendered_base}"
    );
}
