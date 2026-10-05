use crate::ast::{AstHunk, StructuralDiff};

pub fn estimate_code_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut tokens = 0;
    let mut chars = text.chars().peekable();
    let mut in_identifier = false;
    let mut prev_char_lowercase = false;

    while let Some(ch) = chars.next() {
        if ch == '\n' {
            tokens += 1;
            in_identifier = false;
            prev_char_lowercase = false;
            let mut space_count = 0;
            while let Some(&next_ch) = chars.peek() {
                if next_ch == ' ' {
                    space_count += 1;
                    chars.next();
                } else if next_ch == '\t' {
                    space_count += 4;
                    chars.next();
                } else {
                    break;
                }
            }
            if space_count > 0 {
                tokens += (space_count + 3) / 4;
            }
            continue;
        }

        if ch.is_whitespace() {
            in_identifier = false;
            prev_char_lowercase = false;
            continue;
        }

        if ch.is_alphanumeric() {
            if !in_identifier {
                tokens += 1;
                in_identifier = true;
            } else if ch.is_uppercase() && prev_char_lowercase {
                tokens += 1;
            }
            prev_char_lowercase = ch.is_lowercase();
        } else if ch == '_' {
            tokens += 1;
            in_identifier = false;
            prev_char_lowercase = false;
        } else {
            tokens += 1;
            in_identifier = false;
            prev_char_lowercase = false;
        }
    }

    tokens.max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBudgeter {
    pub max_context: usize,
    pub system_prompt_reserve: usize,
    pub generation_reserve: usize,
    pub max_diff_budget: usize,
}

impl Default for TokenBudgeter {
    fn default() -> Self {
        Self::new(4096, 450, 1024)
    }
}

impl TokenBudgeter {
    pub fn new(max_context: usize, system_prompt_reserve: usize, generation_reserve: usize) -> Self {
        let reserved = system_prompt_reserve + generation_reserve;
        let max_diff_budget = if max_context > reserved {
            max_context - reserved
        } else {
            max_context / 2
        };
        Self {
            max_context,
            system_prompt_reserve,
            generation_reserve,
            max_diff_budget,
        }
    }

    pub fn default_for_model(_model: &str) -> Self {
        Self::default()
    }

    pub fn estimate_tokens(&self, text: &str) -> usize {
        estimate_code_tokens(text)
    }

    pub fn fits_budget(&self, text: &str) -> bool {
        self.estimate_tokens(text) <= self.max_diff_budget
    }

    pub fn remaining_capacity(&self, text: &str) -> usize {
        let estimated = self.estimate_tokens(text);
        if self.max_diff_budget > estimated {
            self.max_diff_budget - estimated
        } else {
            0
        }
    }
}

pub fn truncate_monster_node(node_text: &str, max_tokens: usize) -> String {
    let current_tokens = estimate_code_tokens(node_text);
    if current_tokens <= max_tokens {
        return node_text.to_string();
    }

    let lines: Vec<&str> = node_text.lines().collect();
    if lines.len() <= 20 {
        let max_chars = max_tokens * 3;
        if node_text.len() > max_chars {
            let half = max_chars / 2;
            let start = &node_text[..half];
            let end = &node_text[node_text.len() - half..];
            return format!(
                "{}\n    [... interior body truncated to preserve 4K context budget ...]\n{}",
                start, end
            );
        }
        return node_text.to_string();
    }

    let head_count = 12.min(lines.len());
    let tail_count = 6.min(lines.len().saturating_sub(head_count));

    let mut result = String::new();
    for line in &lines[..head_count] {
        result.push_str(line);
        result.push('\n');
    }
    result.push_str("    [... interior body truncated to preserve 4K context budget ...]\n");
    for line in &lines[lines.len() - tail_count..] {
        result.push_str(line);
        result.push('\n');
    }

    result.trim_end().to_string()
}

pub fn partition_diffs_by_budget(
    diffs: &[StructuralDiff],
    budgeter: &TokenBudgeter,
) -> Vec<Vec<StructuralDiff>> {
    if diffs.is_empty() {
        return Vec::new();
    }

    let max_node_tokens = budgeter.max_diff_budget / 2;
    let bounded_diffs: Vec<StructuralDiff> = diffs
        .iter()
        .map(|diff| {
            let bounded_hunks: Vec<AstHunk> = diff
                .hunks
                .iter()
                .map(|hunk| {
                    let old_node = hunk
                        .old_node
                        .as_ref()
                        .map(|s| truncate_monster_node(s, max_node_tokens));
                    let new_node = hunk
                        .new_node
                        .as_ref()
                        .map(|s| truncate_monster_node(s, max_node_tokens));
                    AstHunk::new(&hunk.symbol_name, hunk.kind, old_node, new_node)
                })
                .collect();
            StructuralDiff::new(diff.file_path.clone(), diff.language, bounded_hunks)
        })
        .collect();

    let mut chunks: Vec<Vec<StructuralDiff>> = Vec::new();
    let mut current_chunk: Vec<StructuralDiff> = Vec::new();
    let mut current_chunk_tokens = 0;

    for file_diff in bounded_diffs {
        let file_payload = file_diff.to_llm_payload();
        let file_tokens = budgeter.estimate_tokens(&file_payload);

        if current_chunk_tokens + file_tokens <= budgeter.max_diff_budget {
            current_chunk_tokens += file_tokens;
            current_chunk.push(file_diff);
            continue;
        }

        if file_tokens <= budgeter.max_diff_budget {
            if !current_chunk.is_empty() {
                chunks.push(current_chunk);
                current_chunk = Vec::new();
            }
            current_chunk.push(file_diff);
            current_chunk_tokens = file_tokens;
            continue;
        }

        let mut current_file_hunks: Vec<AstHunk> = Vec::new();
        let mut current_file_tokens = 0;

        for hunk in file_diff.hunks {
            let single_diff = StructuralDiff::new(
                file_diff.file_path.clone(),
                file_diff.language,
                vec![hunk.clone()],
            );
            let single_tokens = budgeter.estimate_tokens(&single_diff.to_llm_payload());

            if current_chunk_tokens + current_file_tokens + single_tokens <= budgeter.max_diff_budget {
                current_file_hunks.push(hunk);
                current_file_tokens += single_tokens;
            } else {
                if !current_file_hunks.is_empty() {
                    let partial_diff = StructuralDiff::new(
                        file_diff.file_path.clone(),
                        file_diff.language,
                        current_file_hunks,
                    );
                    current_chunk.push(partial_diff);
                    chunks.push(current_chunk);
                    current_chunk = Vec::new();
                    current_chunk_tokens = 0;
                } else if !current_chunk.is_empty() {
                    chunks.push(current_chunk);
                    current_chunk = Vec::new();
                    current_chunk_tokens = 0;
                }

                current_file_hunks = vec![hunk];
                current_file_tokens = single_tokens;
            }
        }

        if !current_file_hunks.is_empty() {
            let partial_diff = StructuralDiff::new(
                file_diff.file_path.clone(),
                file_diff.language,
                current_file_hunks,
            );
            current_chunk.push(partial_diff);
            current_chunk_tokens += current_file_tokens;
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}

pub fn serialize_chunk_payload(
    chunk: &[StructuralDiff],
    chunk_index: usize,
    total_chunks: usize,
) -> String {
    let mut payload = String::new();
    if total_chunks > 1 {
        payload.push_str(&format!(
            "=== CHUNK [{}/{}] ===\n",
            chunk_index + 1,
            total_chunks
        ));
    }
    for diff in chunk {
        payload.push_str(&diff.to_llm_payload());
        payload.push('\n');
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::AstChangeKind;
    use crate::git::SupportedLanguage;
    use std::path::PathBuf;

    #[test]
    fn test_token_estimation_accuracy() {
        let rust_code = "pub async fn process_data(&self, buffer: &[u8]) -> Result<usize> { let count = buffer.len(); Ok(count) }";
        let csharp_code = "public async Task<int> ProcessDataAsync(byte[] buffer) { var count = buffer.Length; return count; }";
        let python_code = "async def process_data(self, buffer: bytes) -> int:\n    count = len(buffer)\n    return count";

        let rust_tokens = estimate_code_tokens(rust_code);
        let csharp_tokens = estimate_code_tokens(csharp_code);
        let python_tokens = estimate_code_tokens(python_code);

        assert!(rust_tokens >= 18 && rust_tokens <= 45);
        assert!(csharp_tokens >= 18 && csharp_tokens <= 40);
        assert!(python_tokens >= 14 && python_tokens <= 35);
    }

    #[test]
    fn test_budget_threshold_enforcement() {
        let budgeter = TokenBudgeter::default();
        assert_eq!(budgeter.max_diff_budget, 2622);

        let small_code = "fn small() { println!(\"small\"); }\n".repeat(50);
        assert!(budgeter.fits_budget(&small_code));

        let huge_code = "fn huge_function_with_many_operations() { let x = 1; let y = 2; }\n".repeat(300);
        assert!(!budgeter.fits_budget(&huge_code));
    }

    #[test]
    fn test_partitioning_integrity() {
        let budgeter = TokenBudgeter::new(4096, 450, 1024);
        let mut diffs = Vec::new();

        for i in 0..8 {
            let mut hunks = Vec::new();
            for j in 0..4 {
                let old_body = format!("fn func_{}_{}() {{ let a = {}; let b = {}; }}\n", i, j, i, j).repeat(20);
                let new_body = format!("fn func_{}_{}() -> Result<()> {{ let a = {}; let b = {}; Ok(()) }}\n", i, j, i, j).repeat(20);
                hunks.push(AstHunk::new(
                    format!("func_{}_{}", i, j),
                    AstChangeKind::Modified,
                    Some(old_body),
                    Some(new_body),
                ));
            }
            diffs.push(StructuralDiff::new(
                PathBuf::from(format!("src/file_{}.rs", i)),
                SupportedLanguage::Rust,
                hunks,
            ));
        }

        let chunks = partition_diffs_by_budget(&diffs, &budgeter);
        assert!(chunks.len() > 1);

        let mut total_hunks_in_chunks = 0;
        for chunk in &chunks {
            let payload = serialize_chunk_payload(chunk, 0, chunks.len());
            assert!(budgeter.fits_budget(&payload));
            for file in chunk {
                total_hunks_in_chunks += file.hunks.len();
            }
        }

        let original_total_hunks: usize = diffs.iter().map(|d| d.hunks.len()).sum();
        assert_eq!(total_hunks_in_chunks, original_total_hunks);
    }

    #[test]
    fn test_monster_hunk_truncation() {
        let budgeter = TokenBudgeter::default();
        let massive_line = "let item_value = calculation_step_for_large_routine(index_val);\n";
        let monster_body = massive_line.repeat(300);
        assert!(monster_body.len() > 10000);

        let truncated = truncate_monster_node(&monster_body, 500);
        assert!(truncated.contains("[... interior body truncated to preserve 4K context budget ...]"));
        assert!(budgeter.estimate_tokens(&truncated) < budgeter.estimate_tokens(&monster_body));
    }
}
