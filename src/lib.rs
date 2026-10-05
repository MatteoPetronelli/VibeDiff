pub mod ast;
pub mod client;
pub mod git;

pub use ast::{
    diff_changed_file, diff_files, diff_source, get_language, AstChangeKind, AstHunk,
    StructuralDiff,
};
pub use client::{
    parse_sse_line, parse_sse_text, ChatCompletionChunk, ChatCompletionRequest, ChatMessage,
    Choice, Delta, OllamaClient, StreamOptions, SYSTEM_PROMPT,
};
pub use git::{
    discover_repository, discover_repository_at, get_changed_files, get_changed_files_at,
    get_changed_files_from_repo, is_ignored_lockfile, ChangedFile, SupportedLanguage,
};
