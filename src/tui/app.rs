use crate::ast::{AstHunk, StructuralDiff};
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    FileList,
    HunkDiff,
    Analysis,
}

pub struct App {
    pub files: Vec<StructuralDiff>,
    pub selected_file_index: usize,
    pub selected_hunk_index: usize,
    pub analysis_text: String,
    pub is_analyzing: bool,
    pub active_pane: ActivePane,
    pub scroll_offset_analysis: usize,
    pub scroll_offset_diff: usize,
    pub staged_status: Vec<bool>,
    pub status_message: Option<String>,
}

impl App {
    pub fn new(files: Vec<StructuralDiff>, staged_status: Vec<bool>) -> Self {
        Self {
            files,
            selected_file_index: 0,
            selected_hunk_index: 0,
            analysis_text: String::new(),
            is_analyzing: false,
            active_pane: ActivePane::FileList,
            scroll_offset_analysis: 0,
            scroll_offset_diff: 0,
            staged_status,
            status_message: None,
        }
    }

    pub fn from_diffs(files: Vec<StructuralDiff>, repo: Option<&git2::Repository>) -> Self {
        let mut staged_status = Vec::with_capacity(files.len());
        if let Some(r) = repo {
            if let Ok(index) = r.index() {
                for diff in &files {
                    let posix = diff.file_path.to_string_lossy().replace('\\', "/");
                    let is_staged = index.get_path(Path::new(&posix), 0).is_some();
                    staged_status.push(is_staged);
                }
            } else {
                staged_status.resize(files.len(), false);
            }
        } else {
            staged_status.resize(files.len(), false);
        }
        Self::new(files, staged_status)
    }

    pub fn current_file(&self) -> Option<&StructuralDiff> {
        self.files.get(self.selected_file_index)
    }

    pub fn current_hunk(&self) -> Option<&AstHunk> {
        self.current_file()?.hunks.get(self.selected_hunk_index)
    }

    pub fn next_file(&mut self) {
        if self.files.is_empty() {
            return;
        }
        if self.selected_file_index + 1 >= self.files.len() {
            self.selected_file_index = 0;
        } else {
            self.selected_file_index += 1;
        }
        self.selected_hunk_index = 0;
        self.scroll_offset_diff = 0;
    }

    pub fn previous_file(&mut self) {
        if self.files.is_empty() {
            return;
        }
        if self.selected_file_index == 0 {
            self.selected_file_index = self.files.len() - 1;
        } else {
            self.selected_file_index -= 1;
        }
        self.selected_hunk_index = 0;
        self.scroll_offset_diff = 0;
    }

    pub fn next_hunk(&mut self) {
        let hunk_count = self.current_file().map(|f| f.hunks.len()).unwrap_or(0);
        if hunk_count == 0 {
            return;
        }
        if self.selected_hunk_index + 1 >= hunk_count {
            self.selected_hunk_index = 0;
        } else {
            self.selected_hunk_index += 1;
        }
        self.scroll_offset_diff = 0;
    }

    pub fn previous_hunk(&mut self) {
        let hunk_count = self.current_file().map(|f| f.hunks.len()).unwrap_or(0);
        if hunk_count == 0 {
            return;
        }
        if self.selected_hunk_index == 0 {
            self.selected_hunk_index = hunk_count - 1;
        } else {
            self.selected_hunk_index -= 1;
        }
        self.scroll_offset_diff = 0;
    }

    pub fn next_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::FileList => ActivePane::HunkDiff,
            ActivePane::HunkDiff => ActivePane::Analysis,
            ActivePane::Analysis => ActivePane::FileList,
        };
    }

    pub fn previous_pane(&mut self) {
        self.active_pane = match self.active_pane {
            ActivePane::FileList => ActivePane::Analysis,
            ActivePane::HunkDiff => ActivePane::FileList,
            ActivePane::Analysis => ActivePane::HunkDiff,
        };
    }

    pub fn scroll_analysis_up(&mut self) {
        self.scroll_offset_analysis = self.scroll_offset_analysis.saturating_sub(1);
    }

    pub fn scroll_analysis_down(&mut self) {
        self.scroll_offset_analysis = self.scroll_offset_analysis.saturating_add(1);
    }

    pub fn scroll_diff_up(&mut self) {
        self.scroll_offset_diff = self.scroll_offset_diff.saturating_sub(1);
    }

    pub fn scroll_diff_down(&mut self) {
        self.scroll_offset_diff = self.scroll_offset_diff.saturating_add(1);
    }

    pub fn toggle_stage_selected(&mut self, repo: &git2::Repository) -> Result<()> {
        let file_path = match self.current_file() {
            Some(d) => d.file_path.clone(),
            None => return Ok(()),
        };

        let is_currently_staged = self
            .staged_status
            .get(self.selected_file_index)
            .copied()
            .unwrap_or(false);

        let mut index = repo.index()?;
        if is_currently_staged {
            if let Ok(head) = repo.head().and_then(|h| h.peel_to_commit()) {
                let _ = repo.reset_default(Some(head.as_object()), [file_path.as_path()]);
            } else {
                let _ = index.remove_path(&file_path);
                let _ = index.write();
            }
            if let Some(st) = self.staged_status.get_mut(self.selected_file_index) {
                *st = false;
            }
            self.status_message = Some(format!("Unstaged {}", file_path.display()));
        } else {
            index.add_path(&file_path)?;
            index.write()?;
            if let Some(st) = self.staged_status.get_mut(self.selected_file_index) {
                *st = true;
            }
            self.status_message = Some(format!("Staged {}", file_path.display()));
        }

        Ok(())
    }
}
