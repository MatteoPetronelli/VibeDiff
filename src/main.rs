use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use notify::Watcher;
use std::io::Write;
use std::path::Path;
use vibediff::{
    discover_repository, extract_structural_diff, get_staged_files, get_unstaged_files,
    parse_pillar_sections, partition_diffs_by_budget, serialize_chunk_payload,
    OllamaClient, PillarAnalysisReport, TokenBudgeter, VibeDiffJsonReport,
};

#[derive(Parser, Debug, Clone)]
#[command(name = "vd", version, about = "VibeDiff: AI-powered architectural diff engine")]
pub struct Cli {
    #[arg(short, long)]
    pub staged: bool,

    #[arg(short, long, default_value = "bench-reason-4b")]
    pub model: String,

    #[arg(short, long, default_value = "http:\x2F\x2Flocalhost:11434")]
    pub endpoint: String,

    #[arg(short, long)]
    pub watch: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    pub format: String,
}

impl Cli {
    pub fn is_json(&self) -> bool {
        self.json || self.format == "json"
    }
}

pub async fn execute_pipeline(args: &Cli) -> Result<()> {
    let start_time = std::time::Instant::now();
    let mode = if args.staged { "staged" } else { "unstaged" };

    let client = OllamaClient::new(Some(args.endpoint.clone()), Some(args.model.clone()));
    if let Err(e) = client.check_health().await {
        if args.is_json() {
            eprintln!("Error: {}", e);
        } else {
            eprintln!("{}", format!("Error: {}", e).red().bold());
        }
        return Err(e);
    }

    let repo = discover_repository().context("Failed to discover Git repository")?;
    let repo_root = repo
        .workdir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| ".".to_string());

    let changed_files = if args.staged {
        get_staged_files()?
    } else {
        get_unstaged_files()?
    };

    if changed_files.is_empty() {
        if args.is_json() {
            let report = VibeDiffJsonReport::new(
                repo_root,
                mode,
                0,
                &[],
                None,
                start_time.elapsed().as_millis() as u64,
            );
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else if args.staged {
            println!("No modifications detected in staged index.");
        } else {
            println!("No modifications detected in working directory.");
        }
        return Ok(());
    }

    let mut structural_diffs = Vec::new();
    for file in &changed_files {
        if let Ok(diff) = extract_structural_diff(file) {
            if !diff.hunks.is_empty() {
                structural_diffs.push(diff);
            }
        }
    }

    if structural_diffs.is_empty() {
        if args.is_json() {
            let report = VibeDiffJsonReport::new(
                repo_root,
                mode,
                changed_files.len(),
                &[],
                None,
                start_time.elapsed().as_millis() as u64,
            );
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            println!("{}", "No structural AST changes detected.".dimmed());
        }
        return Ok(());
    }

    let budgeter = TokenBudgeter::default_for_model(&args.model);
    let chunks = partition_diffs_by_budget(&structural_diffs, &budgeter);
    let total_chunks = chunks.len();

    if !args.is_json() {
        println!("{}", "=== VibeDiff Architectural Analysis ===".cyan().bold());
        if total_chunks > 1 {
            println!(
                "{}",
                format!(
                    "[!] Diff exceeds 4K context budget. Executing adaptive chunked analysis ({} passes)...",
                    total_chunks
                )
                .yellow()
                .bold()
            );
        }
    }

    let mut merged_analysis = PillarAnalysisReport::default();
    merged_analysis.model = args.model.clone();

    for (idx, chunk) in chunks.iter().enumerate() {
        if !args.is_json() {
            if total_chunks > 1 {
                let chunk_files: Vec<String> = chunk
                    .iter()
                    .map(|d| d.file_path.display().to_string())
                    .collect();
                println!(
                    "{}",
                    format!(
                        "=== Chunk {}/{}: {} ===",
                        idx + 1,
                        total_chunks,
                        chunk_files.join(", ")
                    )
                    .cyan()
                    .bold()
                );
            }
            for diff in chunk {
                let kinds: Vec<String> = diff
                    .hunks
                    .iter()
                    .map(|h| format!("{}: {}", h.symbol_name, h.kind))
                    .collect();
                println!(
                    "{} [{}] -> {}",
                    diff.file_path.display().to_string().green().bold(),
                    diff.language.to_string().magenta(),
                    kinds.join(", ").yellow()
                );
            }
            println!("{}", "----------------------------------------".bright_black());
        }

        let chunk_payload = serialize_chunk_payload(chunk, idx, total_chunks);
        let mut stdout = std::io::stdout();
        let mut chunk_streamed = String::new();

        let result = client
            .analyze_stream(&chunk_payload, |part| {
                if !args.is_json() {
                    print!("{}", part);
                    let _ = stdout.flush();
                } else {
                    chunk_streamed.push_str(part);
                }
            })
            .await;

        match result {
            Ok(raw_text) => {
                let final_chunk_text = if args.is_json() {
                    chunk_streamed
                } else {
                    raw_text
                };
                if args.is_json() {
                    let chunk_report = parse_pillar_sections(&final_chunk_text, &args.model);
                    merged_analysis.merge(&chunk_report);
                } else {
                    println!();
                    println!("{}", "========================================".cyan().bold());
                }
            }
            Err(e) => {
                if args.is_json() {
                    eprintln!("Inference error: {}", e);
                } else {
                    eprintln!("{}", format!("\nInference error: {}", e).red().bold());
                }
                return Err(e);
            }
        }
    }

    if args.is_json() {
        let report = VibeDiffJsonReport::new(
            repo_root,
            mode,
            changed_files.len(),
            &structural_diffs,
            Some(merged_analysis),
            start_time.elapsed().as_millis() as u64,
        );
        println!("{}", serde_json::to_string_pretty(&report)?);
    }

    Ok(())
}

pub async fn run_once(args: &Cli) -> Result<()> {
    if let Err(err) = execute_pipeline(args).await {
        if args.is_json() {
            eprintln!("Error: {}", err);
        } else {
            eprintln!("{}", format!("Error: {}", err).red().bold());
        }
        std::process::exit(1);
    }
    Ok(())
}

fn should_ignore_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    if path_str.contains(".git") || path_str.contains("target") {
        return true;
    }
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        if file_name.starts_with('.') {
            return true;
        }
        if matches!(
            file_name,
            "Cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock"
        ) {
            return true;
        }
    }
    false
}

fn should_process_event(event: &notify::Event) -> bool {
    if !matches!(
        event.kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Modify(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Any
    ) {
        return false;
    }
    for path in &event.paths {
        if !should_ignore_path(path) {
            return true;
        }
    }
    false
}

pub async fn run_watch(args: &Cli) -> Result<()> {
    if !args.is_json() {
        println!("{}", "Starting VibeDiff in watch mode...".cyan().bold());
    }
    let _ = execute_pipeline(args).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    let mut watcher = notify::RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = tx.blocking_send(event);
            }
        },
        notify::Config::default(),
    )?;

    watcher.watch(Path::new("."), notify::RecursiveMode::Recursive)?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                if !args.is_json() {
                    println!("{}", "\nWatch mode stopped.".yellow());
                }
                break;
            }
            event_opt = rx.recv() => {
                if let Some(event) = event_opt {
                    if should_process_event(&event) {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        while rx.try_recv().is_ok() {}
                        if !args.is_json() {
                            println!("{}", "\n[Change detected - re-analyzing...]".blue().bold());
                        }
                        let _ = execute_pipeline(args).await;
                    }
                } else {
                    break;
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if args.watch {
        run_watch(&args).await
    } else {
        run_once(&args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_argument_parsing_defaults() {
        let args = Cli::parse_from(["vd"]);
        assert!(!args.staged);
        assert!(!args.watch);
        assert!(!args.json);
        assert_eq!(args.format, "text");
        assert_eq!(args.model, "bench-reason-4b");
        assert_eq!(args.endpoint, "http:\x2F\x2Flocalhost:11434");
        assert!(!args.is_json());
    }

    #[test]
    fn test_cli_argument_parsing_custom_flags() {
        let args = Cli::parse_from([
            "vd",
            "--staged",
            "--watch",
            "--json",
            "-m",
            "custom-model",
            "-e",
            "http:\x2F\x2F127.0.0.1:8080",
        ]);
        assert!(args.staged);
        assert!(args.watch);
        assert!(args.json);
        assert!(args.is_json());
        assert_eq!(args.model, "custom-model");
        assert_eq!(args.endpoint, "http:\x2F\x2F127.0.0.1:8080");

        let args_format = Cli::parse_from(["vd", "--format", "json"]);
        assert_eq!(args_format.format, "json");
        assert!(args_format.is_json());
    }

    #[test]
    fn test_path_filter_rules() {
        assert!(should_ignore_path(Path::new(".git/HEAD")));
        assert!(should_ignore_path(Path::new("target/debug/deps")));
        assert!(should_ignore_path(Path::new("Cargo.lock")));
        assert!(should_ignore_path(Path::new(".gitignore")));
        assert!(!should_ignore_path(Path::new("src/main.rs")));
        assert!(!should_ignore_path(Path::new("src/git.rs")));
    }
}
