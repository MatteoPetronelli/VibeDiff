pub mod ast;
pub mod budget;
pub mod client;
pub mod git;
pub mod output;
pub mod tui;

pub use ast::{
    diff_changed_file, diff_conflict_file, diff_files, diff_source, extract_structural_diff,
    get_language, AstChangeKind, AstHunk, StructuralDiff,
};
pub use budget::{
    estimate_code_tokens, partition_diffs_by_budget, serialize_chunk_payload,
    truncate_monster_node, TokenBudgeter,
};
pub use client::{
    parse_sse_line, parse_sse_text, ChatCompletionChunk, ChatCompletionRequest, ChatMessage,
    Choice, Delta, OllamaClient, StreamOptions, CONFLICT_SYSTEM_PROMPT, SYSTEM_PROMPT,
};
pub use git::{
    discover_repository, discover_repository_at, get_changed_files, get_changed_files_at,
    get_changed_files_from_repo, get_changed_files_selective, get_conflict_changes_from_repo,
    get_staged_files, get_unstaged_files, is_ignored_lockfile, split_conflict_markers,
    ChangedFile, ConflictFile, GitDiffExtractor, SupportedLanguage,
};
pub use output::{
    parse_pillar_sections, AstHunkReport, FileStructuralDiffReport, PillarAnalysisReport,
    VibeDiffJsonReport,
};
pub use tokio_util::sync::CancellationToken;
pub use tui::{run_tui, ActivePane, App};
