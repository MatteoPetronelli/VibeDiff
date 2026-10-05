use crate::ast::{AstHunk, StructuralDiff};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AstHunkReport {
    pub symbol_name: String,
    pub kind: String,
    pub old_node: Option<String>,
    pub new_node: Option<String>,
}

impl From<&AstHunk> for AstHunkReport {
    fn from(hunk: &AstHunk) -> Self {
        Self {
            symbol_name: hunk.symbol_name.clone(),
            kind: hunk.kind.to_string(),
            old_node: hunk.old_node.clone(),
            new_node: hunk.new_node.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileStructuralDiffReport {
    pub path: String,
    pub language: String,
    pub hunks: Vec<AstHunkReport>,
}

impl From<&StructuralDiff> for FileStructuralDiffReport {
    fn from(diff: &StructuralDiff) -> Self {
        Self {
            path: diff.file_path.display().to_string(),
            language: diff.language.to_string(),
            hunks: diff.hunks.iter().map(AstHunkReport::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PillarAnalysisReport {
    pub model: String,
    pub data_journey: String,
    pub architectural_patterns: String,
    pub language_caveats: String,
    pub critical_anchors: String,
    pub raw_output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VibeDiffJsonReport {
    pub version: String,
    pub repository_root: String,
    pub mode: String,
    pub files_evaluated: usize,
    pub total_hunks: usize,
    pub diffs: Vec<FileStructuralDiffReport>,
    pub analysis: Option<PillarAnalysisReport>,
    pub execution_time_ms: u64,
}

impl VibeDiffJsonReport {
    pub fn new(
        repository_root: impl Into<String>,
        mode: impl Into<String>,
        files_evaluated: usize,
        diffs: &[StructuralDiff],
        analysis: Option<PillarAnalysisReport>,
        execution_time_ms: u64,
    ) -> Self {
        let diff_reports: Vec<FileStructuralDiffReport> =
            diffs.iter().map(FileStructuralDiffReport::from).collect();
        let total_hunks = diff_reports.iter().map(|d| d.hunks.len()).sum();
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            repository_root: repository_root.into(),
            mode: mode.into(),
            files_evaluated,
            total_hunks,
            diffs: diff_reports,
            analysis,
            execution_time_ms,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum PillarSection {
    None,
    DataJourney,
    ArchitecturalPatterns,
    LanguageCaveats,
    CriticalAnchors,
}

pub fn parse_pillar_sections(raw_text: &str, model: &str) -> PillarAnalysisReport {
    let mut data_journey = String::new();
    let mut architectural_patterns = String::new();
    let mut language_caveats = String::new();
    let mut critical_anchors = String::new();

    let mut current_section = PillarSection::None;

    for line in raw_text.lines() {
        let trimmed = line.trim();
        let normalized = trimmed
            .trim_start_matches(|c: char| c == '#' || c == '*' || c == '-' || c.is_whitespace())
            .trim();
        let upper = normalized.to_ascii_uppercase();

        let detected_section = if upper.contains("1. THE DATA JOURNEY")
            || upper.contains("1. DATA JOURNEY")
            || (upper.starts_with("1.") && upper.contains("DATA JOURNEY"))
            || (upper.starts_with("1)") && upper.contains("DATA JOURNEY"))
            || (upper.starts_with("THE DATA JOURNEY"))
        {
            Some(PillarSection::DataJourney)
        } else if upper.contains("2. ARCHITECTURAL PATTERN")
            || upper.contains("2. DESIGN INTENT")
            || (upper.starts_with("2.") && (upper.contains("ARCHITECT") || upper.contains("DESIGN INTENT")))
            || (upper.starts_with("2)") && (upper.contains("ARCHITECT") || upper.contains("DESIGN INTENT")))
            || upper.starts_with("ARCHITECTURAL PATTERN")
        {
            Some(PillarSection::ArchitecturalPatterns)
        } else if upper.contains("3. LANGUAGE & FRAMEWORK CAVEATS")
            || upper.contains("3. LANGUAGE AND FRAMEWORK CAVEATS")
            || (upper.starts_with("3.") && upper.contains("CAVEAT"))
            || (upper.starts_with("3)") && upper.contains("CAVEAT"))
            || upper.starts_with("LANGUAGE & FRAMEWORK CAVEATS")
            || upper.starts_with("LANGUAGE AND FRAMEWORK CAVEATS")
        {
            Some(PillarSection::LanguageCaveats)
        } else if upper.contains("4. CRITICAL ANCHORS")
            || upper.contains("4. UNHANDLED EDGE CASES")
            || (upper.starts_with("4.") && (upper.contains("ANCHOR") || upper.contains("EDGE CASE")))
            || (upper.starts_with("4)") && (upper.contains("ANCHOR") || upper.contains("EDGE CASE")))
            || upper.starts_with("CRITICAL ANCHORS")
        {
            Some(PillarSection::CriticalAnchors)
        } else {
            None
        };

        if let Some(new_section) = detected_section {
            current_section = new_section;
            if let Some(colon_pos) = trimmed.find(':') {
                let after_colon = trimmed[colon_pos + 1..].trim();
                if !after_colon.is_empty() {
                    match current_section {
                        PillarSection::DataJourney => {
                            data_journey.push_str(after_colon);
                            data_journey.push('\n');
                        }
                        PillarSection::ArchitecturalPatterns => {
                            architectural_patterns.push_str(after_colon);
                            architectural_patterns.push('\n');
                        }
                        PillarSection::LanguageCaveats => {
                            language_caveats.push_str(after_colon);
                            language_caveats.push('\n');
                        }
                        PillarSection::CriticalAnchors => {
                            critical_anchors.push_str(after_colon);
                            critical_anchors.push('\n');
                        }
                        PillarSection::None => {}
                    }
                }
            }
            continue;
        }

        match current_section {
            PillarSection::DataJourney => {
                data_journey.push_str(line);
                data_journey.push('\n');
            }
            PillarSection::ArchitecturalPatterns => {
                architectural_patterns.push_str(line);
                architectural_patterns.push('\n');
            }
            PillarSection::LanguageCaveats => {
                language_caveats.push_str(line);
                language_caveats.push('\n');
            }
            PillarSection::CriticalAnchors => {
                critical_anchors.push_str(line);
                critical_anchors.push('\n');
            }
            PillarSection::None => {}
        }
    }

    PillarAnalysisReport {
        model: model.to_string(),
        data_journey: data_journey.trim().to_string(),
        architectural_patterns: architectural_patterns.trim().to_string(),
        language_caveats: language_caveats.trim().to_string(),
        critical_anchors: critical_anchors.trim().to_string(),
        raw_output: raw_text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::AstChangeKind;
    use crate::git::SupportedLanguage;
    use std::path::PathBuf;

    #[test]
    fn test_json_serialization_contract() {
        let hunks = vec![
            AstHunk::new(
                "execute",
                AstChangeKind::Modified,
                Some("fn execute() -> bool".to_string()),
                Some("fn execute() -> Result<()>".to_string()),
            ),
            AstHunk::new(
                "as_str",
                AstChangeKind::ContractBroken,
                Some("fn as_str(&self) -> &str".to_string()),
                Some("fn as_str(&self, b: bool) -> &str".to_string()),
            ),
        ];

        let diff = StructuralDiff::new(
            PathBuf::from("src/runner.rs"),
            SupportedLanguage::Rust,
            hunks,
        );

        let analysis = PillarAnalysisReport {
            model: "bench-reason-4b".to_string(),
            data_journey: "Data enters runner".to_string(),
            architectural_patterns: "Command pattern".to_string(),
            language_caveats: "Borrow bounds".to_string(),
            critical_anchors: "Missing null check".to_string(),
            raw_output: "1. THE DATA JOURNEY...".to_string(),
        };

        let report = VibeDiffJsonReport::new(
            "C:/projects/test",
            "unstaged",
            1,
            &[diff],
            Some(analysis),
            125,
        );

        let json_str = serde_json::to_string(&report).expect("Failed to serialize report");
        let deserialized: VibeDiffJsonReport =
            serde_json::from_str(&json_str).expect("Failed to deserialize report");

        assert_eq!(report, deserialized);
        assert_eq!(deserialized.total_hunks, 2);
        assert_eq!(deserialized.diffs[0].hunks[1].kind, "CONTRACT_BROKEN");
        assert_eq!(deserialized.mode, "unstaged");
    }

    #[test]
    fn test_pillar_heading_segmentation() {
        let sample = "\
1. THE DATA JOURNEY
The payload enters through HTTP endpoint. It transforms into CST and exits to stdout.

2. ARCHITECTURAL PATTERN & DESIGN INTENT
Applies pipeline pattern with clear boundaries.

3. LANGUAGE & FRAMEWORK CAVEATS
Rust async streams require tokio buffer flush.

4. CRITICAL ANCHORS & UNHANDLED EDGE CASES
Watch loop may drop events if channel is full.
";
        let parsed = parse_pillar_sections(sample, "bench-reason-4b");

        assert_eq!(parsed.model, "bench-reason-4b");
        assert!(parsed.data_journey.contains("The payload enters through HTTP endpoint"));
        assert!(parsed.architectural_patterns.contains("Applies pipeline pattern"));
        assert!(parsed.language_caveats.contains("Rust async streams require tokio buffer flush"));
        assert!(parsed.critical_anchors.contains("Watch loop may drop events"));
        assert_eq!(parsed.raw_output, sample);
    }

    #[test]
    fn test_zero_ast_changes_json_emission() {
        let diffs: Vec<StructuralDiff> = Vec::new();
        let report = VibeDiffJsonReport::new("C:/test/repo", "staged", 0, &diffs, None, 4);

        assert_eq!(report.files_evaluated, 0);
        assert_eq!(report.total_hunks, 0);
        assert!(report.diffs.is_empty());
        assert!(report.analysis.is_none());

        let json = serde_json::to_string_pretty(&report).expect("Serialization failed");
        assert!(json.contains("\"total_hunks\": 0"));
        assert!(json.contains("\"diffs\": []"));
        assert!(json.contains("\"files_evaluated\": 0"));
    }
}
