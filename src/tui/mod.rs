pub mod app;
pub mod ui;
#[cfg(test)]
pub mod tests;

pub use app::{ActivePane, App};
pub use ui::render;

use crate::ast::AstChangeKind;
use crate::budget::{partition_diffs_by_budget, serialize_chunk_payload, TokenBudgeter};
use crate::client::{OllamaClient, CONFLICT_SYSTEM_PROMPT, SYSTEM_PROMPT};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

enum AnalysisMsg {
    Chunk(String),
    Finished,
    Error(String),
}

fn trigger_analysis(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AnalysisMsg>,
    active_token: &mut Option<CancellationToken>,
    endpoint: &str,
    model: &str,
) {
    if app.files.is_empty() {
        return;
    }
    if let Some(token) = active_token.take() {
        token.cancel();
    }
    let new_token = CancellationToken::new();
    *active_token = Some(new_token.clone());

    app.is_analyzing = true;
    app.analysis_text.clear();
    app.scroll_offset_analysis = 0;

    let tx_clone = tx.clone();
    let files = app.files.clone();
    let model_str = model.to_string();
    let endpoint_str = endpoint.to_string();
    let token_clone = new_token;
    let is_conflict = files
        .iter()
        .any(|f| f.hunks.iter().any(|h| h.kind == AstChangeKind::ConflictContested));
    let sys_prompt = if is_conflict {
        CONFLICT_SYSTEM_PROMPT
    } else {
        SYSTEM_PROMPT
    };

    tokio::spawn(async move {
        let client = OllamaClient::new(Some(endpoint_str), Some(model_str.clone()));
        let budgeter = TokenBudgeter::default_for_model(&model_str);
        let chunks = partition_diffs_by_budget(&files, &budgeter);
        let total_chunks = chunks.len();

        for (idx, chunk) in chunks.iter().enumerate() {
            if token_clone.is_cancelled() {
                return;
            }
            let chunk_payload = serialize_chunk_payload(chunk, idx, total_chunks);
            let tx_for_chunk = tx_clone.clone();
            let res = client
                .analyze_stream_cancellable_with_system(
                    &chunk_payload,
                    sys_prompt,
                    token_clone.clone(),
                    move |part| {
                        let _ = tx_for_chunk.send(AnalysisMsg::Chunk(part.to_string()));
                    },
                )
                .await;

            if let Err(e) = res {
                if !token_clone.is_cancelled() {
                    let _ = tx_clone.send(AnalysisMsg::Error(e.to_string()));
                }
                return;
            }
        }
        let _ = tx_clone.send(AnalysisMsg::Finished);
    });
}

pub async fn run_tui(
    mut app: App,
    repo_path: &Path,
    endpoint: &str,
    model: &str,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        default_hook(panic_info);
    }));

    let result = run_tui_inner(&mut terminal, &mut app, repo_path, endpoint, model).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_tui_inner<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    repo_path: &Path,
    endpoint: &str,
    model: &str,
) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AnalysisMsg>();
    let mut active_cancel_token: Option<CancellationToken> = None;

    if !app.files.is_empty() {
        trigger_analysis(app, &tx, &mut active_cancel_token, endpoint, model);
    }

    loop {
        terminal.draw(|f| ui::render(f, app))?;

        while let Ok(msg) = rx.try_recv() {
            match msg {
                AnalysisMsg::Chunk(chunk) => {
                    app.analysis_text.push_str(&chunk);
                }
                AnalysisMsg::Finished => {
                    app.is_analyzing = false;
                }
                AnalysisMsg::Error(err) => {
                    app.is_analyzing = false;
                    app.analysis_text.push_str(&format!("\nError: {}", err));
                }
            }
        }

        if event::poll(Duration::from_millis(40))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                        KeyCode::Tab => app.next_pane(),
                        KeyCode::BackTab => app.previous_pane(),
                        KeyCode::Char('j') | KeyCode::Down => match app.active_pane {
                            ActivePane::FileList => app.next_file(),
                            ActivePane::HunkDiff => app.next_hunk(),
                            ActivePane::Analysis => app.scroll_analysis_down(),
                        },
                        KeyCode::Char('k') | KeyCode::Up => match app.active_pane {
                            ActivePane::FileList => app.previous_file(),
                            ActivePane::HunkDiff => app.previous_hunk(),
                            ActivePane::Analysis => app.scroll_analysis_up(),
                        },
                        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            match app.active_pane {
                                ActivePane::FileList => app.next_file(),
                                ActivePane::HunkDiff => app.scroll_diff_down(),
                                ActivePane::Analysis => app.scroll_analysis_down(),
                            }
                        }
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            match app.active_pane {
                                ActivePane::FileList => app.previous_file(),
                                ActivePane::HunkDiff => app.scroll_diff_up(),
                                ActivePane::Analysis => app.scroll_analysis_up(),
                            }
                        }
                        KeyCode::Char(' ') => {
                            if let Ok(repo) = git2::Repository::open(repo_path) {
                                let _ = app.toggle_stage_selected(&repo);
                            }
                        }
                        KeyCode::Char('r') => {
                            trigger_analysis(app, &tx, &mut active_cancel_token, endpoint, model);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if let Some(token) = active_cancel_token.take() {
        token.cancel();
    }

    Ok(())
}
