use go_html_template::Template;
use std::time::Instant;

fn bench<F: FnMut()>(name: &str, loops: usize, mut f: F) {
    let mut total_us: u128 = 0;
    for _ in 0..loops {
        let start = Instant::now();
        f();
        total_us += start.elapsed().as_micros();
    }
    println!(
        "{name:26} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn main() {
    let mut expr_20k = String::new();
    for _ in 0..20_000 {
        expr_20k.push_str("{{.X}}");
    }

    let mut html_mix = String::new();
    for _ in 0..8_000 {
        html_mix.push_str("<a href=\"{{.U}}\">{{.T}}</a>");
    }

    let parser = Template::new("p");

    bench("parse_tree_expr_20k", 100, || {
        let _ = parser
            .parse_tree(&expr_20k)
            .expect("parse_tree should succeed");
    });

    bench("parse_expr_20k", 80, || {
        let _ = Template::new("p")
            .parse(&expr_20k)
            .expect("parse should succeed");
    });

    bench("parse_tree_html_mix", 80, || {
        let _ = parser
            .parse_tree(&html_mix)
            .expect("parse_tree should succeed");
    });

    bench("parse_html_mix", 60, || {
        let _ = Template::new("p")
            .parse(&html_mix)
            .expect("parse should succeed");
    });
}
