use go_html_template::Template;
use serde_json::json;
use std::time::Instant;

fn main() {
    let mut src_url = String::new();
    for _ in 0..1_000 {
        src_url.push_str("<a href=\"{{.U}}\">x</a>");
    }

    let p0 = Instant::now();
    let t_url = Template::new("t")
        .parse(&src_url)
        .expect("parse should succeed");
    println!("parse_url_us={}", p0.elapsed().as_micros());

    let input_url = json!({"U": "https://example.com/a b?x=1&y=2"});
    let p1 = Instant::now();
    let out = t_url
        .execute_to_string(&input_url)
        .expect("execute should succeed");
    println!("exec_url_us={} len={}", p1.elapsed().as_micros(), out.len());

    let mut src_title = String::new();
    for _ in 0..1_000 {
        src_title.push_str("<a title=\"{{.T}}\">x</a>");
    }

    let p2 = Instant::now();
    let t_title = Template::new("t")
        .parse(&src_title)
        .expect("parse should succeed");
    println!("parse_title_us={}", p2.elapsed().as_micros());

    let input_title = json!({"T": "a < b & c"});
    let p3 = Instant::now();
    let out2 = t_title
        .execute_to_string(&input_title)
        .expect("execute should succeed");
    println!(
        "exec_title_us={} len={}",
        p3.elapsed().as_micros(),
        out2.len()
    );
}
