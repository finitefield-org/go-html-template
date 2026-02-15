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
        "{name:28} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn main() {
    let loops = 40usize;
    let input_items = json!({"Items": (0..20_000).collect::<Vec<_>>()});

    let inline = Template::new("t")
        .parse("{{range .Items}}<li>{{.}}</li>{{end}}")
        .expect("parse should succeed");
    bench("exec_range_inline", loops, || {
        let _ = inline.execute_to_string(&input_items).expect("execute");
    });

    let with_call = Template::new("t")
        .parse("{{define \"item\"}}<li>{{.}}</li>{{end}}{{range .Items}}{{template \"item\" .}}{{end}}")
        .expect("parse should succeed");
    bench("exec_range_template_call", loops, || {
        let _ = with_call.execute_to_string(&input_items).expect("execute");
    });
}
