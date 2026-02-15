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
    let loops_parse_small = 500;
    let loops_parse_large = 60;
    let loops_exec = 60;

    bench("parse_small", loops_parse_small, || {
        let _ = Template::new("p")
            .parse("<p>{{.Name}}</p>")
            .expect("parse should succeed");
    });

    let mut large_expr = String::new();
    for _ in 0..20_000 {
        large_expr.push_str("{{.X}}");
    }
    bench("parse_expr_20k", loops_parse_large, || {
        let _ = Template::new("p")
            .parse(&large_expr)
            .expect("parse should succeed");
    });

    let static_200k = "<ul>".to_string() + &"<li>x</li>".repeat(20_000) + "</ul>";
    let tmpl_text = Template::new("t")
        .parse(&static_200k)
        .expect("parse should succeed");
    let input_empty = json!({});
    bench("exec_text_200k", loops_exec, || {
        let _ = tmpl_text
            .execute_to_string(&input_empty)
            .expect("execute should succeed");
    });

    let mut expr_20k = String::new();
    for _ in 0..20_000 {
        expr_20k.push_str("{{.X}}");
    }
    let tmpl_expr = Template::new("t")
        .parse(&expr_20k)
        .expect("parse should succeed");
    let input_x = json!({"X": "abc"});
    bench("exec_expr_20k", loops_exec, || {
        let _ = tmpl_expr
            .execute_to_string(&input_x)
            .expect("execute should succeed");
    });

    let mut deep_20k = String::new();
    for _ in 0..20_000 {
        deep_20k.push_str("{{.A.B.C.D.E}}\n");
    }
    let tmpl_deep = Template::new("t")
        .parse(&deep_20k)
        .expect("parse should succeed");
    let input_deep = json!({"A": {"B": {"C": {"D": {"E": "abc"}}}}});
    bench("exec_deep_path_20k", loops_exec, || {
        let _ = tmpl_deep
            .execute_to_string(&input_deep)
            .expect("execute should succeed");
    });

    let tmpl_range_no_vars = Template::new("t")
        .parse("<ul>{{range .Items}}<li>x</li>{{end}}</ul>")
        .expect("parse should succeed");
    let input_items = json!({"Items": (0..20_000).collect::<Vec<_>>()});
    bench("exec_range_no_vars", loops_exec, || {
        let _ = tmpl_range_no_vars
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let tmpl_range_vars = Template::new("t")
        .parse("{{range $i, $v := .Items}}{{$i}}:{{$v}};{{end}}")
        .expect("parse should succeed");
    bench("exec_range_with_vars", loops_exec, || {
        let _ = tmpl_range_vars
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let mut func_20k = String::new();
    for _ in 0..20_000 {
        func_20k.push_str("{{print .X}}");
    }
    let tmpl_func = Template::new("t")
        .parse(&func_20k)
        .expect("parse should succeed");
    bench("exec_builtin_func_20k", loops_exec, || {
        let _ = tmpl_func
            .execute_to_string(&input_x)
            .expect("execute should succeed");
    });

    bench("serde_to_value_only", loops_exec * 20, || {
        let _ = serde_json::to_value(&input_items).expect("to_value should succeed");
    });
}
