use go_html_template::Template;
use serde_json::json;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, loops: usize, mut f: F) {
    let mut total_us: u128 = 0;
    for _ in 0..loops {
        let start = Instant::now();
        f();
        total_us += start.elapsed().as_micros();
    }
    println!(
        "{name:30} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn main() {
    let loops = 60usize;
    let input_items = json!({"Items": (0..20_000).collect::<Vec<_>>()});

    let inline_static = Template::new("t")
        .parse("{{range .Items}}<li>x</li>{{end}}")
        .expect("parse should succeed");
    bench("exec_range_inline_static", loops, || {
        let _ = inline_static
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let call_static = Template::new("t")
        .parse("{{define \"item\"}}<li>x</li>{{end}}{{range .Items}}{{template \"item\" .}}{{end}}")
        .expect("parse should succeed");
    bench("exec_range_template_static", loops, || {
        let _ = call_static
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let inline_empty = Template::new("t")
        .parse("{{range .Items}}{{end}}")
        .expect("parse should succeed");
    bench("exec_range_inline_empty", loops, || {
        let _ = inline_empty
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let call_empty = Template::new("t")
        .parse("{{define \"item\"}}{{end}}{{range .Items}}{{template \"item\" .}}{{end}}")
        .expect("parse should succeed");
    bench("exec_range_template_empty", loops, || {
        let _ = call_empty
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });
}
