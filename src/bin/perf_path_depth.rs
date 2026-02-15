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
        "{name:26} loops={loops:4} avg_us={}",
        total_us / loops as u128
    );
}

fn repeated(expr: &str, n: usize) -> String {
    let mut src = String::new();
    for _ in 0..n {
        src.push_str(expr);
    }
    src
}

fn main() {
    let loops = 60usize;
    let n = 20_000usize;

    let t0 = Template::new("t")
        .parse(&repeated("{{.E}}", n))
        .expect("parse should succeed");
    let t1 = Template::new("t")
        .parse(&repeated("{{.A.E}}", n))
        .expect("parse should succeed");
    let t3 = Template::new("t")
        .parse(&repeated("{{.A.B.C.E}}", n))
        .expect("parse should succeed");
    let t5 = Template::new("t")
        .parse(&repeated("{{.A.B.C.D.E}}", n))
        .expect("parse should succeed");

    let data0 = json!({"E": "abc"});
    let data1 = json!({"A": {"E": "abc"}});
    let data3 = json!({"A": {"B": {"C": {"E": "abc"}}}});
    let data5 = json!({"A": {"B": {"C": {"D": {"E": "abc"}}}}});

    bench("path_depth0", loops, || {
        let _ = t0
            .execute_to_string(&data0)
            .expect("execute should succeed");
    });
    bench("path_depth1", loops, || {
        let _ = t1
            .execute_to_string(&data1)
            .expect("execute should succeed");
    });
    bench("path_depth3", loops, || {
        let _ = t3
            .execute_to_string(&data3)
            .expect("execute should succeed");
    });
    bench("path_depth5", loops, || {
        let _ = t5
            .execute_to_string(&data5)
            .expect("execute should succeed");
    });
}
