use go_html_template::Template;
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_TEMPLATE: &str = "{{range $i, $v := .Items}}{{$i}}:{{$v}};{{end}}";
const DEFAULT_ITEMS: usize = 20_000;

#[derive(Debug)]
struct Args {
    template: Option<PathBuf>,
    data: Option<PathBuf>,
    loops: usize,
    missingkey: String,
    go_bin: String,
    go_runner: PathBuf,
}

#[derive(Debug, Deserialize)]
struct GoReport {
    parse_avg_us: u64,
    exec_avg_us: u64,
    output: String,
    output_len: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let missingkey_option = format!("missingkey={}", args.missingkey);

    let (template_path, template_temp) = prepare_template_path(args.template.as_deref())?;
    let (data_path, data_temp) = prepare_data_path(args.data.as_deref())?;

    let template_source = fs::read_to_string(&template_path)?;
    let data_source = fs::read_to_string(&data_path)?;
    let data: serde_json::Value = serde_json::from_str(&data_source)?;

    let rust_parse_avg_us = bench_rust_parse(&template_source, &missingkey_option, args.loops)?;
    let (rust_exec_avg_us, rust_output) =
        bench_rust_exec(&template_source, &data, &missingkey_option, args.loops)?;

    let go_report = run_go(
        &args.go_bin,
        &args.go_runner,
        &template_path,
        &data_path,
        &args.missingkey,
        args.loops,
    )?;

    let output_match = rust_output == go_report.output;

    println!("template={}", template_path.display());
    println!("data={}", data_path.display());
    println!("loops={}", args.loops);
    println!("missingkey={}", args.missingkey);
    println!();
    println!("{:<24} avg_us={}", "rust_parse", rust_parse_avg_us);
    println!("{:<24} avg_us={}", "rust_execute", rust_exec_avg_us);
    println!("{:<24} avg_us={}", "go_parse", go_report.parse_avg_us);
    println!("{:<24} avg_us={}", "go_execute", go_report.exec_avg_us);
    println!();
    println!("{:<24} {}", "rust_output_len", rust_output.len());
    println!("{:<24} {}", "go_output_len", go_report.output_len);
    println!("{:<24} {}", "output_match", output_match);

    if !output_match {
        println!(
            "{:<24} {}",
            "first_diff_byte",
            first_diff_byte_index(&rust_output, &go_report.output)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        println!(
            "{:<24} {:?}",
            "rust_preview",
            preview_for_log(&rust_output, 200)
        );
        println!(
            "{:<24} {:?}",
            "go_preview",
            preview_for_log(&go_report.output, 200)
        );
    }

    drop(template_temp);
    drop(data_temp);

    Ok(())
}

fn parse_args() -> Result<Args, Box<dyn std::error::Error>> {
    let mut args = Args {
        template: None,
        data: None,
        loops: 60,
        missingkey: "default".to_string(),
        go_bin: "go".to_string(),
        go_runner: PathBuf::from("tools/compare_go_html_template/main.go"),
    };

    let mut iter = env::args().skip(1);
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--template" => {
                args.template = Some(PathBuf::from(next_value(&mut iter, "--template")?))
            }
            "--data" => args.data = Some(PathBuf::from(next_value(&mut iter, "--data")?)),
            "--loops" => {
                let value = next_value(&mut iter, "--loops")?;
                args.loops = value.parse::<usize>()?;
            }
            "--missingkey" => args.missingkey = next_value(&mut iter, "--missingkey")?,
            "--go-bin" => args.go_bin = next_value(&mut iter, "--go-bin")?,
            "--go-runner" => args.go_runner = PathBuf::from(next_value(&mut iter, "--go-runner")?),
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                return Err(format!("unknown flag: {flag}").into());
            }
        }
    }

    if args.loops == 0 {
        return Err("--loops must be greater than zero".into());
    }
    if !matches!(
        args.missingkey.as_str(),
        "default" | "invalid" | "zero" | "error"
    ) {
        return Err("--missingkey must be one of default|invalid|zero|error".into());
    }

    Ok(args)
}

fn next_value(
    iter: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    iter.next()
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn print_help() {
    println!("Compare this crate and Go html/template with the same template/data.");
    println!();
    println!("Usage:");
    println!("  cargo run --release --bin compare_go_html_template -- [options]");
    println!();
    println!("Options:");
    println!("  --template <path>    template file path (default: built-in range template)");
    println!("  --data <path>        JSON data path (default: built-in Items[0..20000))");
    println!("  --loops <n>          benchmark loops for parse/execute (default: 60)");
    println!("  --missingkey <mode>  default|invalid|zero|error (default: default)");
    println!("  --go-bin <path>      go binary path (default: go)");
    println!(
        "  --go-runner <path>   go runner path (default: tools/compare_go_html_template/main.go)"
    );
}

fn prepare_template_path(
    path: Option<&Path>,
) -> Result<(PathBuf, Option<TempFileGuard>), Box<dyn std::error::Error>> {
    if let Some(path) = path {
        return Ok((path.to_path_buf(), None));
    }

    let temp = TempFileGuard::new("template", DEFAULT_TEMPLATE)?;
    Ok((temp.path.clone(), Some(temp)))
}

fn prepare_data_path(
    path: Option<&Path>,
) -> Result<(PathBuf, Option<TempFileGuard>), Box<dyn std::error::Error>> {
    if let Some(path) = path {
        return Ok((path.to_path_buf(), None));
    }

    let data = json!({"Items": (0..DEFAULT_ITEMS).collect::<Vec<_>>()});
    let json_text = serde_json::to_string(&data)?;
    let temp = TempFileGuard::new("data", &json_text)?;
    Ok((temp.path.clone(), Some(temp)))
}

fn bench_rust_parse(
    template: &str,
    missingkey_option: &str,
    loops: usize,
) -> Result<u128, Box<dyn std::error::Error>> {
    let start = Instant::now();
    for _ in 0..loops {
        let _ = Template::new("bench")
            .option(missingkey_option)?
            .parse(template)?;
    }
    Ok(start.elapsed().as_micros() / loops as u128)
}

fn bench_rust_exec(
    template: &str,
    data: &serde_json::Value,
    missingkey_option: &str,
    loops: usize,
) -> Result<(u128, String), Box<dyn std::error::Error>> {
    let parsed = Template::new("bench")
        .option(missingkey_option)?
        .parse(template)?;

    let start = Instant::now();
    let mut output = String::new();
    for _ in 0..loops {
        output = parsed.execute_to_string(data)?;
    }
    let avg = start.elapsed().as_micros() / loops as u128;
    Ok((avg, output))
}

fn run_go(
    go_bin: &str,
    go_runner: &Path,
    template_path: &Path,
    data_path: &Path,
    missingkey: &str,
    loops: usize,
) -> Result<GoReport, Box<dyn std::error::Error>> {
    if !go_runner.exists() {
        return Err(format!("go runner not found: {}", go_runner.display()).into());
    }

    let output = Command::new(go_bin)
        .arg("run")
        .arg(go_runner)
        .arg("--template")
        .arg(template_path)
        .arg("--data")
        .arg(data_path)
        .arg("--loops")
        .arg(loops.to_string())
        .arg("--missingkey")
        .arg(missingkey)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "go runner failed (status={}):\nstdout:\n{}\nstderr:\n{}",
            output.status, stdout, stderr
        )
        .into());
    }

    let report: GoReport = serde_json::from_slice(&output.stdout)?;
    Ok(report)
}

fn first_diff_byte_index(left: &str, right: &str) -> Option<usize> {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let min_len = left_bytes.len().min(right_bytes.len());

    for index in 0..min_len {
        if left_bytes[index] != right_bytes[index] {
            return Some(index);
        }
    }

    if left_bytes.len() == right_bytes.len() {
        None
    } else {
        Some(min_len)
    }
}

fn preview_for_log(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

struct TempFileGuard {
    path: PathBuf,
}

impl TempFileGuard {
    fn new(tag: &str, content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut path = env::temp_dir();
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        path.push(format!(
            "go_html_template_compare_{}_{}_{}.tmp",
            tag,
            std::process::id(),
            timestamp
        ));
        fs::write(&path, content)?;
        Ok(Self { path })
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
