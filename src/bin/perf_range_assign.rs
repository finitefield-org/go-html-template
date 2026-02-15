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
        "{name:30} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn main() {
    let loops = 60usize;
    let input_items = json!({"Items": (0..20_000).collect::<Vec<_>>()});

    let t_decl = Template::new("t")
        .parse("{{range $i, $v := .Items}}{{end}}")
        .expect("parse should succeed");
    bench("range_decl_empty", loops, || {
        let _ = t_decl
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let t_assign = Template::new("t")
        .parse("{{$i := 0}}{{$v := 0}}{{range $i, $v = .Items}}{{end}}")
        .expect("parse should succeed");
    bench("range_assign_empty", loops, || {
        let _ = t_assign
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let t_decl_print = Template::new("t")
        .parse("{{range $i, $v := .Items}}{{$i}}:{{$v}};{{end}}")
        .expect("parse should succeed");
    bench("range_decl_print", loops, || {
        let _ = t_decl_print
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });

    let t_assign_print = Template::new("t")
        .parse("{{$i := 0}}{{$v := 0}}{{range $i, $v = .Items}}{{$i}}:{{$v}};{{end}}")
        .expect("parse should succeed");
    bench("range_assign_print", loops, || {
        let _ = t_assign_print
            .execute_to_string(&input_items)
            .expect("execute should succeed");
    });
}
