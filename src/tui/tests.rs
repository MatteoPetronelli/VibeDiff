use super::*;
use crate::ast::{AstChangeKind, AstHunk, StructuralDiff};
use crate::git::SupportedLanguage;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn create_dummy_diff(name: &str, hunk_count: usize) -> StructuralDiff {
    let hunks = (0..hunk_count)
        .map(|i| {
            AstHunk::new(
                format!("fn_{}_{}", name, i),
                AstChangeKind::Modified,
                Some(format!("old_{}", i)),
                Some(format!("new_{}", i)),
            )
        })
        .collect();
    StructuralDiff::new(PathBuf::from(name), SupportedLanguage::Rust, hunks)
}

#[test]
fn test_app_navigation_and_wrapping() {
    let diffs = vec![
        create_dummy_diff("file_a.rs", 2),
        create_dummy_diff("file_b.rs", 3),
        create_dummy_diff("file_c.rs", 1),
    ];
    let mut app = App::new(diffs, vec![false, false, false]);

    assert_eq!(app.selected_file_index, 0);
    app.next_file();
    assert_eq!(app.selected_file_index, 1);
    app.next_file();
    assert_eq!(app.selected_file_index, 2);
    app.next_file();
    assert_eq!(app.selected_file_index, 0);

    app.previous_file();
    assert_eq!(app.selected_file_index, 2);
    app.previous_file();
    assert_eq!(app.selected_file_index, 1);
    app.previous_file();
    assert_eq!(app.selected_file_index, 0);
}

#[test]
fn test_pane_cycling() {
    let diffs = vec![create_dummy_diff("file_a.rs", 1)];
    let mut app = App::new(diffs, vec![false]);

    assert_eq!(app.active_pane, ActivePane::FileList);
    app.next_pane();
    assert_eq!(app.active_pane, ActivePane::HunkDiff);
    app.next_pane();
    assert_eq!(app.active_pane, ActivePane::Analysis);
    app.next_pane();
    assert_eq!(app.active_pane, ActivePane::FileList);

    app.previous_pane();
    assert_eq!(app.active_pane, ActivePane::Analysis);
    app.previous_pane();
    assert_eq!(app.active_pane, ActivePane::HunkDiff);
    app.previous_pane();
    assert_eq!(app.active_pane, ActivePane::FileList);
}

#[test]
fn test_hunk_selection_binding() {
    let diffs = vec![
        create_dummy_diff("file_a.rs", 2),
        create_dummy_diff("file_b.rs", 3),
    ];
    let mut app = App::new(diffs, vec![false, false]);

    assert_eq!(app.selected_hunk_index, 0);
    assert_eq!(app.current_hunk().unwrap().symbol_name, "fn_file_a.rs_0");

    app.next_hunk();
    assert_eq!(app.selected_hunk_index, 1);
    assert_eq!(app.current_hunk().unwrap().symbol_name, "fn_file_a.rs_1");

    app.next_hunk();
    assert_eq!(app.selected_hunk_index, 0);

    app.previous_hunk();
    assert_eq!(app.selected_hunk_index, 1);

    app.next_file();
    assert_eq!(app.selected_file_index, 1);
    assert_eq!(app.selected_hunk_index, 0);
    assert_eq!(app.current_hunk().unwrap().symbol_name, "fn_file_b.rs_0");

    app.previous_hunk();
    assert_eq!(app.selected_hunk_index, 2);
    assert_eq!(app.current_hunk().unwrap().symbol_name, "fn_file_b.rs_2");
}

#[test]
fn test_git_staging_toggle_logic() {
    let unique_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("vibediff_tui_test_{}", unique_id));
    fs::create_dir_all(&temp_dir).unwrap();

    let repo = git2::Repository::init(&temp_dir).unwrap();
    let file_rel = "stage_test.rs";
    let file_full = temp_dir.join(file_rel);
    fs::write(&file_full, "fn hello() {}\n").unwrap();

    let diff = StructuralDiff::new(
        PathBuf::from(file_rel),
        SupportedLanguage::Rust,
        vec![AstHunk::new("hello", AstChangeKind::Added, None, Some("fn hello() {}".to_string()))],
    );

    let mut app = App::new(vec![diff], vec![false]);
    assert_eq!(app.staged_status[0], false);

    let stage_res = app.toggle_stage_selected(&repo);
    assert!(stage_res.is_ok());
    assert_eq!(app.staged_status[0], true);

    let index = repo.index().unwrap();
    assert!(index.get_path(Path::new(file_rel), 0).is_some());

    let unstage_res = app.toggle_stage_selected(&repo);
    assert!(unstage_res.is_ok());
    assert_eq!(app.staged_status[0], false);

    let _ = fs::remove_dir_all(&temp_dir);
}
