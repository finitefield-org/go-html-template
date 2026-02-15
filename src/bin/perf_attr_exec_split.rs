use go_html_template::Template;
use serde_json::json;
use std::time::Instant;

fn bench_exec(label: &str, t: &Template, data: serde_json::Value, loops: usize) {
    let mut total_us: u128 = 0;
    for _ in 0..loops {
        let p = Instant::now();
        let _ = t.execute_to_string(&data).expect("execute should succeed");
        total_us += p.elapsed().as_micros();
    }
    println!(
        "{label:26} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn main() {
    let loops = 120usize;

    let mut src_url = String::new();
    for _ in 0..1_000 {
        src_url.push_str("<a href=\"{{.U}}\">x</a>");
    }
    let t_url = Template::new("t")
        .parse(&src_url)
        .expect("parse should succeed");

    bench_exec("url_simple", &t_url, json!({"U": "abc"}), loops);
    bench_exec(
        "url_with_query",
        &t_url,
        json!({"U": "https://example.com/a?x=1&y=2"}),
        loops,
    );
    bench_exec(
        "url_with_spaces",
        &t_url,
        json!({"U": "https://example.com/a b?x=1&y=2"}),
        loops,
    );
    bench_exec(
        "url_blocked_scheme",
        &t_url,
        json!({"U": "javascript:alert(1)"}),
        loops,
    );

    let mut src_title = String::new();
    for _ in 0..1_000 {
        src_title.push_str("<a title=\"{{.T}}\">x</a>");
    }
    let t_title = Template::new("t")
        .parse(&src_title)
        .expect("parse should succeed");

    bench_exec("title_plain", &t_title, json!({"T": "abc"}), loops);
    bench_exec("title_escaped", &t_title, json!({"T": "a < b & c"}), loops);
}
