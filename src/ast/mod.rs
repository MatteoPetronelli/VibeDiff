use crate::git::{ChangedFile, SupportedLanguage};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstChangeKind {
    Modified,
    Added,
    Deleted,
    ContractBroken,
}

impl AstChangeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Modified => "MODIFIED",
            Self::Added => "ADDED",
            Self::Deleted => "DELETED",
            Self::ContractBroken => "CONTRACT_BROKEN",
        }
    }
}

impl std::fmt::Display for AstChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstHunk {
    pub symbol_name: String,
    pub kind: AstChangeKind,
    pub old_node: Option<String>,
    pub new_node: Option<String>,
}

impl AstHunk {
    pub fn new(
        symbol_name: impl Into<String>,
        kind: AstChangeKind,
        old_node: Option<String>,
        new_node: Option<String>,
    ) -> Self {
        Self {
            symbol_name: symbol_name.into(),
            kind,
            old_node,
            new_node,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralDiff {
    pub file_path: PathBuf,
    pub language: SupportedLanguage,
    pub hunks: Vec<AstHunk>,
}

impl StructuralDiff {
    pub fn new(file_path: PathBuf, language: SupportedLanguage, hunks: Vec<AstHunk>) -> Self {
        Self {
            file_path,
            language,
            hunks,
        }
    }

    pub fn to_llm_payload(&self) -> String {
        let mut payload = String::new();
        payload.push_str(&format!(
            "FILE: {} [{}]\n",
            self.file_path.display(),
            self.language
        ));
        if self.hunks.is_empty() {
            payload.push_str("NO_STRUCTURAL_CHANGES\n");
            return payload;
        }
        for hunk in &self.hunks {
            payload.push_str(&format!(
                "--- SYMBOL: {} [{}] ---\n",
                hunk.symbol_name, hunk.kind
            ));
            if let Some(old) = &hunk.old_node {
                payload.push_str("<<< OLD\n");
                payload.push_str(old.trim());
                payload.push('\n');
            }
            if let Some(new) = &hunk.new_node {
                payload.push_str(">>> NEW\n");
                payload.push_str(new.trim());
                payload.push('\n');
            }
        }
        payload
    }
}

pub fn get_language(language: SupportedLanguage) -> Option<tree_sitter::Language> {
    match language {
        SupportedLanguage::Rust => Some(tree_sitter_rust::language()),
        SupportedLanguage::CSharp => Some(tree_sitter_c_sharp::language()),
        SupportedLanguage::Python => Some(tree_sitter_python::language()),
        SupportedLanguage::Cpp => Some(tree_sitter_cpp::language()),
        SupportedLanguage::TypeScript => Some(tree_sitter_typescript::language_typescript()),
        SupportedLanguage::JavaScript => Some(tree_sitter_javascript::language()),
        SupportedLanguage::Go => Some(tree_sitter_go::language()),
    }
}

struct ExtractedNode {
    symbol_name: String,
    signature_tokens: Vec<String>,
    all_tokens: Vec<String>,
    clean_text: String,
}

fn is_comment_node(kind: &str) -> bool {
    kind == "comment"
        || kind == "line_comment"
        || kind == "block_comment"
        || kind.contains("comment")
}

fn is_leaf_functional_node(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_declaration"
            | "method_definition"
            | "local_function_statement"
            | "constructor_declaration"
    )
}

fn is_functional_node(kind: &str) -> bool {
    is_leaf_functional_node(kind)
        || matches!(
            kind,
            "struct_item"
                | "enum_item"
                | "trait_item"
                | "class_declaration"
                | "struct_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "type_alias_declaration"
                | "class_specifier"
                | "struct_specifier"
                | "match_expression"
                | "match_statement"
                | "switch_statement"
        )
}

fn contains_leaf_functional(node: tree_sitter::Node) -> bool {
    if is_leaf_functional_node(node.kind()) {
        return true;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if contains_leaf_functional(child) {
                return true;
            }
        }
    }
    false
}

fn extract_declarator_identifier(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier" | "qualified_identifier"
    ) {
        if let Ok(text) = std::str::from_utf8(&source[node.byte_range()]) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    if let Some(decl) = node.child_by_field_name("declarator") {
        if let Some(name) = extract_declarator_identifier(decl, source) {
            return Some(name);
        }
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if let Some(name) = extract_declarator_identifier(child, source) {
                return Some(name);
            }
        }
    }
    None
}

fn get_symbol_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(name) = std::str::from_utf8(&source[name_node.byte_range()]) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    if let Some(decl_node) = node.child_by_field_name("declarator") {
        if let Some(name) = extract_declarator_identifier(decl_node, source) {
            return Some(name);
        }
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if matches!(
                child.kind(),
                "identifier" | "type_identifier" | "field_identifier" | "property_identifier"
            ) {
                if let Ok(text) = std::str::from_utf8(&source[child.byte_range()]) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    if matches!(node.kind(), "match_expression" | "match_statement") {
        return Some("match".to_string());
    }
    if matches!(node.kind(), "switch_statement" | "switch_expression") {
        return Some("switch".to_string());
    }

    None
}

fn get_body_range(node: tree_sitter::Node) -> Option<(usize, usize)> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some((body.start_byte(), body.end_byte()));
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if matches!(
                child.kind(),
                "block" | "statement_block" | "compound_statement" | "declaration_list"
            ) {
                return Some((child.start_byte(), child.end_byte()));
            }
        }
    }
    None
}

fn collect_tokens(
    node: tree_sitter::Node,
    source: &[u8],
    is_sig_only: bool,
    body_range: Option<(usize, usize)>,
    tokens: &mut Vec<String>,
) {
    if is_comment_node(node.kind()) {
        return;
    }

    if is_sig_only {
        if let Some((b_start, b_end)) = body_range {
            if node.start_byte() >= b_start && node.end_byte() <= b_end {
                return;
            }
        }
    }

    if node.child_count() == 0 {
        if !node.byte_range().is_empty() {
            if let Ok(text) = std::str::from_utf8(&source[node.byte_range()]) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    tokens.push(trimmed.to_string());
                }
            }
        }
        return;
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_tokens(child, source, is_sig_only, body_range, tokens);
        }
    }
}

fn collect_comment_ranges(node: tree_sitter::Node, ranges: &mut Vec<(usize, usize)>) {
    if is_comment_node(node.kind()) {
        ranges.push((node.start_byte(), node.end_byte()));
        return;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_comment_ranges(child, ranges);
        }
    }
}

fn clean_node_text(node: tree_sitter::Node, source: &[u8]) -> String {
    let mut comment_ranges = Vec::new();
    collect_comment_ranges(node, &mut comment_ranges);
    comment_ranges.sort_by_key(|r| r.0);

    let start = node.start_byte();
    let end = node.end_byte();
    let mut result = Vec::new();
    let mut curr = start;

    for (c_start, c_end) in comment_ranges {
        if c_start > curr && c_start <= end {
            result.extend_from_slice(&source[curr..c_start]);
        }
        curr = curr.max(c_end);
    }
    if curr < end {
        result.extend_from_slice(&source[curr..end]);
    }

    let raw = String::from_utf8_lossy(&result);
    let mut cleaned_lines = Vec::new();
    let mut prev_blank = false;
    for line in raw.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.trim().is_empty() {
            if !prev_blank && !cleaned_lines.is_empty() {
                cleaned_lines.push("");
                prev_blank = true;
            }
        } else {
            cleaned_lines.push(trimmed_end);
            prev_blank = false;
        }
    }
    cleaned_lines.join("\n").trim().to_string()
}

fn extract_nodes(
    cursor_node: tree_sitter::Node,
    source: &[u8],
    result: &mut Vec<ExtractedNode>,
) {
    if is_functional_node(cursor_node.kind()) {
        if let Some(symbol_name) = get_symbol_name(cursor_node, source) {
            let mut has_child_functional = false;
            for i in 0..cursor_node.child_count() {
                if let Some(child) = cursor_node.child(i) {
                    if contains_leaf_functional(child) {
                        has_child_functional = true;
                        break;
                    }
                }
            }
            if !has_child_functional {
                let body_range = get_body_range(cursor_node);
                let mut sig_tokens = Vec::new();
                collect_tokens(cursor_node, source, true, body_range, &mut sig_tokens);
                let mut all_tokens = Vec::new();
                collect_tokens(cursor_node, source, false, None, &mut all_tokens);
                let clean_text = clean_node_text(cursor_node, source);
                result.push(ExtractedNode {
                    symbol_name,
                    signature_tokens: sig_tokens,
                    all_tokens,
                    clean_text,
                });
                return;
            }
        }
    }

    for i in 0..cursor_node.child_count() {
        if let Some(child) = cursor_node.child(i) {
            extract_nodes(child, source, result);
        }
    }
}

pub fn diff_source(
    file_path: PathBuf,
    language: SupportedLanguage,
    old_content: &str,
    new_content: &str,
) -> Result<StructuralDiff> {
    let mut parser = tree_sitter::Parser::new();
    let grammar = get_language(language)
        .ok_or_else(|| anyhow!("Unsupported language for AST diffing: {:?}", language))?;
    parser
        .set_language(&grammar)
        .map_err(|e| anyhow!("Failed to set Tree-sitter language: {:?}", e))?;

    let old_tree = if old_content.trim().is_empty() {
        None
    } else {
        Some(
            parser
                .parse(old_content.as_bytes(), None)
                .context("Failed to parse old content with Tree-sitter")?,
        )
    };

    let new_tree = if new_content.trim().is_empty() {
        None
    } else {
        Some(
            parser
                .parse(new_content.as_bytes(), None)
                .context("Failed to parse new content with Tree-sitter")?,
        )
    };

    let mut old_nodes = Vec::new();
    if let Some(tree) = &old_tree {
        extract_nodes(tree.root_node(), old_content.as_bytes(), &mut old_nodes);
    }

    let mut new_nodes = Vec::new();
    if let Some(tree) = &new_tree {
        extract_nodes(tree.root_node(), new_content.as_bytes(), &mut new_nodes);
    }

    let mut matched_new = vec![false; new_nodes.len()];
    let mut hunks = Vec::new();

    for old_node in &old_nodes {
        let mut found_idx = None;
        for (idx, new_node) in new_nodes.iter().enumerate() {
            if !matched_new[idx]
                && new_node.symbol_name == old_node.symbol_name
                && new_node.signature_tokens == old_node.signature_tokens
            {
                found_idx = Some(idx);
                break;
            }
        }
        if found_idx.is_none() {
            for (idx, new_node) in new_nodes.iter().enumerate() {
                if !matched_new[idx] && new_node.symbol_name == old_node.symbol_name {
                    found_idx = Some(idx);
                    break;
                }
            }
        }

        if let Some(idx) = found_idx {
            matched_new[idx] = true;
            let new_node = &new_nodes[idx];
            if old_node.all_tokens == new_node.all_tokens {
                continue;
            }
            let kind = if old_node.signature_tokens != new_node.signature_tokens {
                AstChangeKind::ContractBroken
            } else {
                AstChangeKind::Modified
            };
            hunks.push(AstHunk::new(
                &old_node.symbol_name,
                kind,
                Some(old_node.clean_text.clone()),
                Some(new_node.clean_text.clone()),
            ));
        } else {
            hunks.push(AstHunk::new(
                &old_node.symbol_name,
                AstChangeKind::Deleted,
                Some(old_node.clean_text.clone()),
                None,
            ));
        }
    }

    for (idx, new_node) in new_nodes.iter().enumerate() {
        if !matched_new[idx] {
            hunks.push(AstHunk::new(
                &new_node.symbol_name,
                AstChangeKind::Added,
                None,
                Some(new_node.clean_text.clone()),
            ));
        }
    }

    Ok(StructuralDiff::new(file_path, language, hunks))
}

pub fn diff_changed_file(file: &ChangedFile) -> Result<StructuralDiff> {
    diff_source(
        file.path.clone(),
        file.language,
        &file.old_content,
        &file.new_content,
    )
}

pub fn diff_files(files: &[ChangedFile]) -> Result<Vec<StructuralDiff>> {
    let mut diffs = Vec::with_capacity(files.len());
    for file in files {
        diffs.push(diff_changed_file(file)?);
    }
    Ok(diffs)
}

#[cfg(test)]
mod tests;
