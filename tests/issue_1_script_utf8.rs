use go_html_template::Template;
use serde_json::json;

#[test]
fn utf8_literal_in_script_should_stay_intact() {
    let template = Template::new("t")
        .parse(r#"<script>const degree = "°";</script>"#)
        .expect("parse template");

    let rendered = template
        .execute_to_string(&json!({}))
        .expect("render template");

    assert!(rendered.contains("\"°\""), "got: {rendered}");
}
