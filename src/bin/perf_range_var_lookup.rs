use go_html_template::Template;
use serde_json::json;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, loops: usize, mut f: F) {
    let mut total_us: u128 = 0;
    for _ in 0..loops {
        let p = Instant::now();
        f();
        total_us += p.elapsed().as_micros();
    }
    println!(
        "{name:32} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn main() {
    let loops = 60usize;
    let input_items = json!({"Items": (0..20_000).collect::<Vec<_>>()});

    let t_dot = Template::new("t")
        .parse("{{range .Items}}{{.}}{{end}}")
        .expect("parse should succeed");
    bench("range_dot_print", loops, || {
        let _ = t_dot
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let t_var = Template::new("t")
        .parse("{{range $i, $v := .Items}}{{$v}}{{end}}")
        .expect("parse should succeed");
    bench("range_var_print", loops, || {
        let _ = t_var
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let t_var_idx = Template::new("t")
        .parse("{{range $i, $v := .Items}}{{$i}}{{end}}")
        .expect("parse should succeed");
    bench("range_var_idx_print", loops, || {
        let _ = t_var_idx
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });
}
