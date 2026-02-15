use go_html_template::Template;
use std::time::Instant;

fn bench_parse(name: &str, src: &str, loops: usize) {
    let mut total_us: u128 = 0;
    for _ in 0..loops {
        let p = Instant::now();
        let _ = Template::new("t").parse(src).expect("parse should succeed");
        total_us += p.elapsed().as_micros();
    }
    println!(
        "{name:26} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn repeated(snippet: &str, n: usize) -> String {
    let mut out = String::new();
    for _ in 0..n {
        out.push_str(snippet);
    }
    out
}

fn main() {
    let loops = 80usize;
    let n = 1000usize;

    let href_static = repeated("<a href=\"https://example.com/x\">x</a>", n);
    let href_expr = repeated("<a href=\"{{.U}}\">x</a>", n);
    let title_static = repeated("<a title=\"abc\">x</a>", n);
    let title_expr = repeated("<a title=\"{{.T}}\">x</a>", n);

    bench_parse("parse_href_static", &href_static, loops);
    bench_parse("parse_href_expr", &href_expr, loops);
    bench_parse("parse_title_static", &title_static, loops);
    bench_parse("parse_title_expr", &title_expr, loops);
}
