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
    let loops = 40usize;
    let input_items = json!({"Items": (0..20_000).collect::<Vec<_>>()});

    let t_range_empty = Template::new("t")
        .parse("{{range .Items}}{{end}}")
        .expect("parse should succeed");
    bench("exec_range_empty_no_vars", loops, || {
        let _ = t_range_empty
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let t_range_empty_vars = Template::new("t")
        .parse("{{range $i, $v := .Items}}{{end}}")
        .expect("parse should succeed");
    bench("exec_range_empty_with_vars", loops, || {
        let _ = t_range_empty_vars
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let t_range_print_vars = Template::new("t")
        .parse("{{range $i, $v := .Items}}{{$v}}{{end}}")
        .expect("parse should succeed");
    bench("exec_range_print_with_vars", loops, || {
        let _ = t_range_print_vars
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let mut src_url = String::new();
    for _ in 0..5_000 {
        src_url.push_str("<a href=\"{{.U}}\">x</a>");
    }
    let t_url = Template::new("t")
        .parse(&src_url)
        .expect("parse should succeed");
    let input_url = json!({"U": "https://example.com/a b?x=1&y=2"});
    bench("exec_url_attr_5k", loops, || {
        let _ = t_url
            .execute_to_string(&input_url)
            .expect("execute should succeed");
    });

    let mut src_title = String::new();
    for _ in 0..5_000 {
        src_title.push_str("<a title=\"{{.T}}\">x</a>");
    }
    let t_title = Template::new("t")
        .parse(&src_title)
        .expect("parse should succeed");
    let input_title = json!({"T": "a < b & c"});
    bench("exec_normal_attr_5k", loops, || {
        let _ = t_title
            .execute_to_string(&input_title)
            .expect("execute should succeed");
    });
}
