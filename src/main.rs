use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use notify::Watcher;
use std::io::Write;
use std::path::Path;
use vibediff::{
    discover_repository, extract_structural_diff, get_staged_files, get_unstaged_files,
    OllamaClient,
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
}

pub async fn execute_pipeline(args: &Cli) -> Result<()> {
    let client = OllamaClient::new(Some(args.endpoint.clone()), Some(args.model.clone()));
    client.check_health().await?;

    let _repo = discover_repository().context("Failed to discover Git repository")?;

    let changed_files = if args.staged {
        get_staged_files()?
    } else {
        get_unstaged_files()?
    };

    if changed_files.is_empty() {
        if args.staged {
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
        println!("No structural AST changes detected.");
        return Ok(());
    }

    let mut full_payload = String::new();
    println!("{}", "=== VibeDiff Architectural Analysis ===".cyan().bold());

    for diff in &structural_diffs {
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
        full_payload.push_str(&diff.to_llm_payload());
        full_payload.push('\n');
    }

    println!("{}", "----------------------------------------".bright_black());

    let mut stdout = std::io::stdout();
    let result = client
        .analyze_stream(&full_payload, |chunk| {
            print!("{}", chunk);
            let _ = stdout.flush();
        })
        .await;

    match result {
        Ok(_) => {
            println!();
            println!("{}", "========================================".cyan().bold());
        }
        Err(e) => {
            eprintln!("{}", format!("\nInference error: {}", e).red().bold());
        }
    }

    Ok(())
}

pub async fn run_once(args: &Cli) -> Result<()> {
    if let Err(err) = execute_pipeline(args).await {
        eprintln!("{}", format!("Error: {}", err).red().bold());
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
    println!("{}", "Starting VibeDiff in watch mode...".cyan().bold());
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
                println!("{}", "\nWatch mode stopped.".yellow());
                break;
            }
            event_opt = rx.recv() => {
                if let Some(event) = event_opt {
                    if should_process_event(&event) {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        while rx.try_recv().is_ok() {}
                        println!("{}", "\n[Change detected - re-analyzing...]".blue().bold());
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
        assert_eq!(args.model, "bench-reason-4b");
        assert_eq!(args.endpoint, "http:\x2F\x2Flocalhost:11434");
    }

    #[test]
    fn test_cli_argument_parsing_custom_flags() {
        let args = Cli::parse_from([
            "vd",
            "--staged",
            "--watch",
            "-m",
            "custom-model",
            "-e",
            "http:\x2F\x2F127.0.0.1:8080",
        ]);
        assert!(args.staged);
        assert!(args.watch);
        assert_eq!(args.model, "custom-model");
        assert_eq!(args.endpoint, "http:\x2F\x2F127.0.0.1:8080");
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
