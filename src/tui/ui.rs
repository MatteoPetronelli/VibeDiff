use crate::ast::AstChangeKind;
use crate::tui::app::{ActivePane, App};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render(frame: &mut Frame, app: &App) {
    let size = frame.size();
    if size.width < 10 || size.height < 10 {
        return;
    }

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(35),
            Constraint::Length(1),
        ])
        .split(size);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(65),
        ])
        .split(main_chunks[0]);

    render_file_list(frame, app, top_chunks[0]);
    render_hunk_diff(frame, app, top_chunks[1]);
    render_analysis(frame, app, main_chunks[1]);
    render_footer(frame, app, main_chunks[2]);
}

fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == ActivePane::FileList;
    let border_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title = if is_active {
        " Files [*] "
    } else {
        " Files "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);

    if app.files.is_empty() {
        let empty_msg = Paragraph::new("No files changed")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        frame.render_widget(empty_msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .files
        .iter()
        .enumerate()
        .map(|(idx, file)| {
            let is_staged = app.staged_status.get(idx).copied().unwrap_or(false);
            let stage_span = if is_staged {
                Span::styled("[+] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("[ ] ", Style::default().fg(Color::DarkGray))
            };

            let path_str = file.file_path.display().to_string();
            let is_selected = idx == app.selected_file_index;
            let path_style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let count_span = Span::styled(
                format!(" ({})", file.hunks.len()),
                Style::default().fg(Color::DarkGray),
            );

            let line = Line::from(vec![stage_span, Span::styled(path_str, path_style), count_span]);
            let item = ListItem::new(line);
            if is_selected {
                item.style(Style::default().bg(Color::Rgb(30, 35, 45)))
            } else {
                item
            }
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_hunk_diff(frame: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == ActivePane::HunkDiff;
    let border_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let hunk_count = app.current_file().map(|f| f.hunks.len()).unwrap_or(0);
    let title = if hunk_count > 0 {
        format!(
            " Hunk Diff ({}/{}){} ",
            app.selected_hunk_index + 1,
            hunk_count,
            if is_active { " [*]" } else { "" }
        )
    } else if is_active {
        " Hunk Diff [*] ".to_string()
    } else {
        " Hunk Diff ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let hunk = match app.current_hunk() {
        Some(h) => h,
        None => {
            let msg = if app.files.is_empty() {
                "No changes"
            } else {
                "No structural hunks for this file"
            };
            let p = Paragraph::new(msg)
                .style(Style::default().fg(Color::DarkGray))
                .block(block);
            frame.render_widget(p, area);
            return;
        }
    };

    let (kind_color, kind_str) = match hunk.kind {
        AstChangeKind::Added => (Color::Green, "ADDED"),
        AstChangeKind::Deleted => (Color::Red, "DELETED"),
        AstChangeKind::Modified => (Color::Yellow, "MODIFIED"),
        AstChangeKind::ContractBroken => (Color::LightRed, "CONTRACT_BROKEN"),
        AstChangeKind::ConflictContested => (Color::Magenta, "CONFLICT_CONTESTED"),
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Symbol: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&hunk.symbol_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled("Kind: ", Style::default().fg(Color::DarkGray)),
        Span::styled(kind_str, Style::default().fg(kind_color).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(Span::styled(
        "--------------------------------------------------",
        Style::default().fg(Color::DarkGray),
    )));

    match hunk.kind {
        AstChangeKind::Added => {
            if let Some(new_code) = &hunk.new_node {
                for line in new_code.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("+ ", Style::default().fg(Color::Green)),
                        Span::styled(line, Style::default().fg(Color::Green)),
                    ]));
                }
            }
        }
        AstChangeKind::Deleted => {
            if let Some(old_code) = &hunk.old_node {
                for line in old_code.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("- ", Style::default().fg(Color::Red)),
                        Span::styled(line, Style::default().fg(Color::Red)),
                    ]));
                }
            }
        }
        AstChangeKind::ConflictContested => {
            lines.push(Line::from(Span::styled(
                "<<<<<<< BASE / OURS",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            )));
            if let Some(old_code) = &hunk.old_node {
                for line in old_code.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("< ", Style::default().fg(Color::Magenta)),
                        Span::styled(line, Style::default().fg(Color::LightMagenta)),
                    ]));
                }
            }
            lines.push(Line::from(Span::styled(
                "=======",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                ">>>>>>> THEIRS / INCOMING",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            if let Some(new_code) = &hunk.new_node {
                for line in new_code.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("> ", Style::default().fg(Color::Cyan)),
                        Span::styled(line, Style::default().fg(Color::LightCyan)),
                    ]));
                }
            }
        }
        AstChangeKind::Modified | AstChangeKind::ContractBroken => {
            if let Some(old_code) = &hunk.old_node {
                lines.push(Line::from(Span::styled(
                    "--- Previous AST Node ---",
                    Style::default().fg(Color::DarkGray),
                )));
                for line in old_code.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("- ", Style::default().fg(Color::Red)),
                        Span::styled(line, Style::default().fg(Color::LightRed)),
                    ]));
                }
            }
            if let Some(new_code) = &hunk.new_node {
                lines.push(Line::from(Span::styled(
                    "+++ Modified AST Node +++",
                    Style::default().fg(Color::DarkGray),
                )));
                for line in new_code.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("+ ", Style::default().fg(Color::Green)),
                        Span::styled(line, Style::default().fg(Color::LightGreen)),
                    ]));
                }
            }
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((app.scroll_offset_diff as u16, 0));
    frame.render_widget(paragraph, area);
}

fn render_analysis(frame: &mut Frame, app: &App, area: Rect) {
    let is_active = app.active_pane == ActivePane::Analysis;
    let border_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    let title = if app.is_analyzing {
        " 4-Pillar Architectural Analysis [Streaming...] "
    } else if is_active {
        " 4-Pillar Architectural Analysis [*] "
    } else {
        " 4-Pillar Architectural Analysis "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);

    if app.analysis_text.is_empty() {
        let msg = if app.is_analyzing {
            "Contacting Ollama and streaming architectural analysis..."
        } else {
            "No analysis loaded. Press 'r' to analyze with Ollama."
        };
        let p = Paragraph::new(msg)
            .style(Style::default().fg(Color::Yellow))
            .block(block);
        frame.render_widget(p, area);
        return;
    }

    let lines: Vec<Line> = app
        .analysis_text
        .lines()
        .map(|line| {
            if line.starts_with('#') {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("Pillar")
                || line.starts_with("1.")
                || line.starts_with("2.")
                || line.starts_with("3.")
                || line.starts_with("4.")
            {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(line, Style::default().fg(Color::White)))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset_analysis as u16, 0));
    frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled("[j/k] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Navigate  "),
        Span::styled("[Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Pane  "),
        Span::styled("[Space] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Stage/Unstage  "),
        Span::styled("[r] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Re-analyze  "),
        Span::styled("[q/Esc] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("Quit"),
    ];

    if let Some(msg) = &app.status_message {
        spans.push(Span::raw("  |  "));
        spans.push(Span::styled(
            msg,
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ));
    }

    let footer = Paragraph::new(Line::from(spans));
    frame.render_widget(footer, area);
}
