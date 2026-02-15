use go_html_template::Template;
use serde_json::json;
use std::time::Instant;

fn build_template_set(connected_count: usize, unrelated_count: usize) -> String {
    let mut source = String::new();
    source.push_str("{{define \"root\"}}{{template \"t0\" .}}{{end}}");

    for i in 0..connected_count {
        if i + 1 < connected_count {
            source.push_str(&format!(
                "{{{{define \"t{i}\"}}}}<a href=\"/p{i}/{{{{.U}}}}?q={i}\">{{{{template \"t{}\" .}}}}</a>{{{{end}}}}",
                i + 1
            ));
        } else {
            source.push_str(&format!(
                "{{{{define \"t{i}\"}}}}<span>{{{{.X}}}}</span>{{{{end}}}}"
            ));
        }
    }

    for i in 0..unrelated_count {
        source.push_str(&format!(
            "{{{{define \"u{i}\"}}}}<section data-id=\"{i}\">{{{{.X}}}}-{{{{.U}}}}</section>{{{{end}}}}"
        ));
    }

    source
}

fn build_single_update(index: usize, iteration: usize, connected_count: usize) -> String {
    let next = if index + 1 < connected_count {
        format!("{{{{template \"t{}\" .}}}}", index + 1)
    } else {
        "{{.X}}".to_string()
    };

    format!(
        "{{{{define \"t{index}\"}}}}<a href=\"/p{index}/{{{{.U}}}}?v={iteration}\">{next}</a>{{{{end}}}}"
    )
}

fn main() {
    let connected_count = 40usize;
    let unrelated_count = 400usize;
    let parse_loops = 400usize;
    let exec_loops = 1200usize;
    let update_index = connected_count / 2;

    let initial_source = build_template_set(connected_count, unrelated_count);
    let mut template = Template::new("root")
        .parse(&initial_source)
        .expect("initial parse should succeed");

    let mut parse_total_us: u128 = 0;
    for i in 0..parse_loops {
        let patch = build_single_update(update_index, i, connected_count);
        let start = Instant::now();
        template = template
            .parse(&patch)
            .expect("incremental parse should succeed");
        parse_total_us += start.elapsed().as_micros();
    }

    let data = json!({"U": "abc", "X": "xyz"});
    let mut exec_total_us: u128 = 0;
    for _ in 0..exec_loops {
        let start = Instant::now();
        let _ = template
            .execute_to_string(&data)
            .expect("execute should succeed");
        exec_total_us += start.elapsed().as_micros();
    }

    println!("connected_count={connected_count}");
    println!("unrelated_count={unrelated_count}");
    println!("update_index={update_index}");
    println!("parse_loops={parse_loops}");
    println!("exec_loops={exec_loops}");
    println!("parse_update_avg_us={}", parse_total_us / parse_loops as u128);
    println!("exec_avg_us={}", exec_total_us / exec_loops as u128);
}
