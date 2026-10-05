use anyhow::{anyhow, Context, Result};
use git2::Repository;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    CSharp,
    Cpp,
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
}

impl SupportedLanguage {
    pub fn from_extension(ext: &str) -> Option<Self> {
        let clean_ext = ext.strip_prefix('.').unwrap_or(ext).to_ascii_lowercase();
        match clean_ext.as_str() {
            "cs" => Some(Self::CSharp),
            "cpp" => Some(Self::Cpp),
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "ts" => Some(Self::TypeScript),
            "js" => Some(Self::JavaScript),
            "go" => Some(Self::Go),
            _ => None,
        }
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> Option<Self> {
        let ext = path.as_ref().extension()?.to_str()?;
        Self::from_extension(ext)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CSharp => "csharp",
            Self::Cpp => "cpp",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Go => "go",
        }
    }
}

impl std::fmt::Display for SupportedLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub language: SupportedLanguage,
    pub old_content: String,
    pub new_content: String,
}

impl ChangedFile {
    pub fn new(
        path: PathBuf,
        language: SupportedLanguage,
        old_content: String,
        new_content: String,
    ) -> Self {
        Self {
            path,
            language,
            old_content,
            new_content,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: PathBuf,
    pub language: SupportedLanguage,
    pub ancestor: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub working_copy: String,
}

impl ConflictFile {
    pub fn new(
        path: PathBuf,
        language: SupportedLanguage,
        ancestor: Option<String>,
        ours: Option<String>,
        theirs: Option<String>,
        working_copy: String,
    ) -> Self {
        Self {
            path,
            language,
            ancestor,
            ours,
            theirs,
            working_copy,
        }
    }
}

pub fn is_ignored_lockfile(path: &Path) -> bool {
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        matches!(
            file_name,
            "Cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock"
        )
    } else {
        false
    }
}

pub fn discover_repository() -> Result<Repository> {
    let current_dir = std::env::current_dir()
        .context("Failed to determine current working directory")?;
    discover_repository_at(&current_dir)
}

pub fn discover_repository_at<P: AsRef<Path>>(path: P) -> Result<Repository> {
    let path_ref = path.as_ref();
    Repository::discover(path_ref)
        .with_context(|| format!("Failed to discover Git repository at {:?}", path_ref))
}

pub fn get_changed_files() -> Result<Vec<ChangedFile>> {
    let repo = discover_repository()?;
    get_changed_files_from_repo(&repo)
}

pub fn get_changed_files_at<P: AsRef<Path>>(path: P) -> Result<Vec<ChangedFile>> {
    let repo = discover_repository_at(path)?;
    get_changed_files_from_repo(&repo)
}

pub fn get_staged_files() -> Result<Vec<ChangedFile>> {
    let repo = discover_repository()?;
    get_changed_files_selective(&repo, Some(true))
}

pub fn get_unstaged_files() -> Result<Vec<ChangedFile>> {
    let repo = discover_repository()?;
    get_changed_files_selective(&repo, Some(false))
}

pub fn get_changed_files_from_repo(repo: &Repository) -> Result<Vec<ChangedFile>> {
    get_changed_files_selective(repo, None)
}

pub fn get_changed_files_selective(
    repo: &Repository,
    staged_filter: Option<bool>,
) -> Result<Vec<ChangedFile>> {
    let workdir = repo
        .workdir()
        .context("Repository does not have a working directory")?;

    let mut status_opts = git2::StatusOptions::new();
    status_opts.include_untracked(true);
    status_opts.recurse_untracked_dirs(true);
    status_opts.include_ignored(false);
    status_opts.include_unmodified(false);

    let statuses = repo
        .statuses(Some(&mut status_opts))
        .context("Failed to retrieve repository statuses")?;

    let head_tree = match repo.head() {
        Ok(head_ref) => match head_ref.peel_to_commit() {
            Ok(commit) => commit.tree().ok(),
            Err(_) => None,
        },
        Err(_) => None,
    };

    let mut changed_files = Vec::new();

    for entry in statuses.iter() {
        let status = entry.status();

        if status.contains(git2::Status::IGNORED) {
            continue;
        }

        if let Some(true) = staged_filter {
            let is_staged = status.intersects(
                git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED
                    | git2::Status::INDEX_RENAMED
                    | git2::Status::INDEX_TYPECHANGE,
            );
            if !is_staged {
                continue;
            }
        }

        if let Some(false) = staged_filter {
            let is_unstaged = status.intersects(
                git2::Status::WT_NEW
                    | git2::Status::WT_MODIFIED
                    | git2::Status::WT_RENAMED
                    | git2::Status::WT_TYPECHANGE,
            );
            if !is_unstaged {
                continue;
            }
        }

        if status.contains(git2::Status::WT_DELETED) || status.contains(git2::Status::INDEX_DELETED) {
            if !status.intersects(
                git2::Status::WT_NEW
                    | git2::Status::WT_MODIFIED
                    | git2::Status::INDEX_NEW
                    | git2::Status::INDEX_MODIFIED,
            ) {
                continue;
            }
        }

        let path_str = match entry.path() {
            Some(p) => p,
            None => continue,
        };

        let rel_path = PathBuf::from(path_str);

        if is_ignored_lockfile(&rel_path) {
            continue;
        }

        let language = match SupportedLanguage::from_path(&rel_path) {
            Some(lang) => lang,
            None => continue,
        };

        let full_path = workdir.join(&rel_path);
        let posix_path = path_str.replace('\\', "/");

        let is_wt_new = status.contains(git2::Status::WT_NEW);
        let is_staged = status.intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE,
        );
        if is_wt_new && !is_staged {
            if full_path.is_file() {
                if let Ok(meta) = std::fs::metadata(&full_path) {
                    if meta.len() > 256 * 1024 {
                        continue;
                    }
                }
            }
            let nesting = rel_path.parent().map(|p| p.components().count()).unwrap_or(0);
            if nesting > 3 {
                continue;
            }
        }

        let new_content = if let Some(true) = staged_filter {
            if let Ok(index) = repo.index() {
                if let Some(entry) = index.get_path(Path::new(&posix_path), 0) {
                    if let Ok(blob) = repo.find_blob(entry.id) {
                        if blob.content().contains(&0) {
                            continue;
                        }
                        String::from_utf8_lossy(blob.content()).to_string()
                    } else if full_path.is_file() {
                        let bytes = std::fs::read(&full_path).map_err(|e| anyhow!(e))?;
                        if bytes.contains(&0) {
                            continue;
                        }
                        String::from_utf8(bytes).map_err(|e| anyhow!(e))?
                    } else {
                        continue;
                    }
                } else if full_path.is_file() {
                    let bytes = std::fs::read(&full_path).map_err(|e| anyhow!(e))?;
                    if bytes.contains(&0) {
                        continue;
                    }
                    String::from_utf8(bytes).map_err(|e| anyhow!(e))?
                } else {
                    continue;
                }
            } else if full_path.is_file() {
                let bytes = std::fs::read(&full_path).map_err(|e| anyhow!(e))?;
                if bytes.contains(&0) {
                    continue;
                }
                String::from_utf8(bytes).map_err(|e| anyhow!(e))?
            } else {
                continue;
            }
        } else {
            if !full_path.is_file() {
                continue;
            }
            let new_bytes = match std::fs::read(&full_path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    return Err(anyhow!(e).context(format!(
                        "Failed to read working directory file: {:?}",
                        full_path
                    )));
                }
            };
            if new_bytes.contains(&0) {
                continue;
            }
            match String::from_utf8(new_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            }
        };

        let old_content = match &head_tree {
            Some(tree) => {
                let lookup_path = entry
                    .head_to_index()
                    .and_then(|diff| diff.old_file().path().and_then(|p| p.to_str()))
                    .unwrap_or(path_str);
                let posix_lookup = lookup_path.replace('\\', "/");
                match tree.get_path(Path::new(&posix_lookup)) {
                    Ok(tree_entry) => {
                        let object = tree_entry.to_object(repo).with_context(|| {
                            format!("Failed to retrieve tree object for {:?}", posix_lookup)
                        })?;
                        if let Some(blob) = object.as_blob() {
                            if blob.content().contains(&0) {
                                continue;
                            }
                            String::from_utf8_lossy(blob.content()).to_string()
                        } else {
                            String::new()
                        }
                    }
                    Err(_) => String::new(),
                }
            }
            None => String::new(),
        };

        changed_files.push(ChangedFile {
            path: rel_path,
            language,
            old_content,
            new_content,
        });
    }

    Ok(changed_files)
}

pub fn split_conflict_markers(source: &str) -> Option<(String, String)> {
    if !source.contains("<<<<<<<") || !source.contains("=======") || !source.contains(">>>>>>>") {
        return None;
    }
    let mut ours = String::new();
    let mut theirs = String::new();
    let mut in_ours = false;
    let mut in_theirs = false;
    let mut found_marker = false;

    for line in source.lines() {
        if line.starts_with("<<<<<<<") {
            in_ours = true;
            in_theirs = false;
            found_marker = true;
        } else if line.starts_with("=======") && in_ours {
            in_ours = false;
            in_theirs = true;
        } else if line.starts_with(">>>>>>>") && in_theirs {
            in_ours = false;
            in_theirs = false;
        } else if in_ours {
            ours.push_str(line);
            ours.push('\n');
        } else if in_theirs {
            theirs.push_str(line);
            theirs.push('\n');
        } else {
            ours.push_str(line);
            ours.push('\n');
            theirs.push_str(line);
            theirs.push('\n');
        }
    }

    if found_marker {
        Some((ours, theirs))
    } else {
        None
    }
}

pub struct GitDiffExtractor<'a> {
    pub repo: &'a Repository,
}

impl<'a> GitDiffExtractor<'a> {
    pub fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    pub fn repo(&self) -> &Repository {
        self.repo
    }

    pub fn has_conflicts(&self) -> Result<bool> {
        let mut has = self.repo.index()?.has_conflicts();
        if !has {
            if let Ok(unstaged) = get_unstaged_files() {
                for file in unstaged {
                    if split_conflict_markers(&file.new_content).is_some() {
                        has = true;
                        break;
                    }
                }
            }
        }
        Ok(has)
    }

    pub fn get_conflict_changes(&self) -> Result<Vec<ConflictFile>> {
        get_conflict_changes_from_repo(self.repo)
    }
}

pub fn get_conflict_changes_from_repo(repo: &Repository) -> Result<Vec<ConflictFile>> {
    let mut conflict_files = Vec::new();
    let mut processed_paths = std::collections::HashSet::new();
    let index = repo.index()?;

    if index.has_conflicts() {
        let conflicts = index.conflicts()?;
        for conflict_res in conflicts {
            let conflict = conflict_res?;
            let path_opt = conflict
                .our
                .as_ref()
                .map(|e| &e.path)
                .or_else(|| conflict.their.as_ref().map(|e| &e.path))
                .or_else(|| conflict.ancestor.as_ref().map(|e| &e.path));

            let path_bytes = match path_opt {
                Some(b) => b,
                None => continue,
            };

            let path_str = match std::str::from_utf8(path_bytes) {
                Ok(s) => s.replace('\\', "/"),
                Err(_) => continue,
            };

            let rel_path = PathBuf::from(&path_str);
            if is_ignored_lockfile(&rel_path) {
                continue;
            }

            let language = match SupportedLanguage::from_path(&rel_path) {
                Some(lang) => lang,
                None => continue,
            };

            let ancestor = read_blob_content(repo, conflict.ancestor.as_ref());
            let ours = read_blob_content(repo, conflict.our.as_ref());
            let theirs = read_blob_content(repo, conflict.their.as_ref());

            let workdir = repo.workdir().context("Repository does not have a working directory")?;
            let full_path = workdir.join(&rel_path);
            let working_copy = if full_path.is_file() {
                std::fs::read_to_string(&full_path).unwrap_or_default()
            } else {
                String::new()
            };

            processed_paths.insert(rel_path.clone());
            conflict_files.push(ConflictFile {
                path: rel_path,
                language,
                ancestor,
                ours,
                theirs,
                working_copy,
            });
        }
    }

    if let Ok(unstaged) = get_unstaged_files() {
        for file in unstaged {
            if !processed_paths.contains(&file.path) {
                if let Some((ours_split, theirs_split)) = split_conflict_markers(&file.new_content) {
                    processed_paths.insert(file.path.clone());
                    conflict_files.push(ConflictFile {
                        path: file.path,
                        language: file.language,
                        ancestor: if !file.old_content.is_empty() {
                            Some(file.old_content)
                        } else {
                            None
                        },
                        ours: Some(ours_split),
                        theirs: Some(theirs_split),
                        working_copy: file.new_content,
                    });
                }
            }
        }
    }

    Ok(conflict_files)
}

fn read_blob_content(repo: &Repository, entry: Option<&git2::IndexEntry>) -> Option<String> {
    let entry = entry?;
    let blob = repo.find_blob(entry.id).ok()?;
    if blob.content().contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(blob.content()).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_language_mapping() {
        assert_eq!(SupportedLanguage::from_extension("cs"), Some(SupportedLanguage::CSharp));
        assert_eq!(SupportedLanguage::from_extension(".cs"), Some(SupportedLanguage::CSharp));
        assert_eq!(SupportedLanguage::from_extension("cpp"), Some(SupportedLanguage::Cpp));
        assert_eq!(SupportedLanguage::from_extension(".cpp"), Some(SupportedLanguage::Cpp));
        assert_eq!(SupportedLanguage::from_extension("rs"), Some(SupportedLanguage::Rust));
        assert_eq!(SupportedLanguage::from_extension(".rs"), Some(SupportedLanguage::Rust));
        assert_eq!(SupportedLanguage::from_extension("py"), Some(SupportedLanguage::Python));
        assert_eq!(SupportedLanguage::from_extension(".py"), Some(SupportedLanguage::Python));
        assert_eq!(SupportedLanguage::from_extension("ts"), Some(SupportedLanguage::TypeScript));
        assert_eq!(SupportedLanguage::from_extension(".ts"), Some(SupportedLanguage::TypeScript));
        assert_eq!(SupportedLanguage::from_extension("js"), Some(SupportedLanguage::JavaScript));
        assert_eq!(SupportedLanguage::from_extension(".js"), Some(SupportedLanguage::JavaScript));
        assert_eq!(SupportedLanguage::from_extension("go"), Some(SupportedLanguage::Go));
        assert_eq!(SupportedLanguage::from_extension(".go"), Some(SupportedLanguage::Go));
        assert_eq!(SupportedLanguage::from_extension("txt"), None);
        assert_eq!(SupportedLanguage::from_extension("exe"), None);
        assert_eq!(SupportedLanguage::from_extension(""), None);

        assert_eq!(SupportedLanguage::from_path(Path::new("src/main.rs")), Some(SupportedLanguage::Rust));
        assert_eq!(SupportedLanguage::from_path(Path::new("scripts/test.py")), Some(SupportedLanguage::Python));
        assert_eq!(SupportedLanguage::from_path(Path::new("Cargo.lock")), None);
    }

    #[test]
    fn test_ignored_lockfiles() {
        assert!(is_ignored_lockfile(Path::new("Cargo.lock")));
        assert!(is_ignored_lockfile(Path::new("subdir/package-lock.json")));
        assert!(is_ignored_lockfile(Path::new("pnpm-lock.yaml")));
        assert!(is_ignored_lockfile(Path::new("yarn.lock")));
        assert!(!is_ignored_lockfile(Path::new("main.rs")));
        assert!(!is_ignored_lockfile(Path::new("Cargo.toml")));
    }

    #[test]
    fn test_active_git_repository_discovery() {
        let repo = discover_repository().expect("should discover current repository");
        assert!(repo.workdir().is_some());
        let changed = get_changed_files().expect("should retrieve changed files from active git state");
        for file in changed {
            assert!(!is_ignored_lockfile(&file.path));
            assert!(!file.new_content.is_empty() || file.old_content.is_empty());
        }
    }

    #[test]
    fn test_synthetic_git_workflow() {
        let temp_dir = std::env::temp_dir().join(format!("vibediff_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let repo = Repository::init(&temp_dir).unwrap();

        let sig = git2::Signature::now("Tester", "tester@example.com").unwrap();
        let file_a_path = temp_dir.join("file_a.rs");
        std::fs::write(&file_a_path, "fn initial() {}\n").unwrap();

        let file_c_path = temp_dir.join("file_c.go");
        std::fs::write(&file_c_path, "package main\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file_a.rs")).unwrap();
        index.add_path(Path::new("file_c.go")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[]).unwrap();

        std::fs::write(&file_a_path, "fn modified_workdir() {}\n").unwrap();

        let file_b_path = temp_dir.join("file_b.py");
        std::fs::write(&file_b_path, "def staged(): pass\n").unwrap();
        index.add_path(Path::new("file_b.py")).unwrap();
        index.write().unwrap();

        let lock_path = temp_dir.join("Cargo.lock");
        std::fs::write(&lock_path, "lockfile content").unwrap();

        let binary_path = temp_dir.join("binary.bin");
        std::fs::write(&binary_path, &[0u8, 159u8, 146u8, 150u8]).unwrap();

        std::fs::remove_file(&file_c_path).unwrap();

        let changed_files = get_changed_files_from_repo(&repo).unwrap();

        let file_a = changed_files.iter().find(|f| f.path == Path::new("file_a.rs")).expect("file_a.rs should be found");
        assert_eq!(file_a.language, SupportedLanguage::Rust);
        assert_eq!(file_a.old_content, "fn initial() {}\n");
        assert_eq!(file_a.new_content, "fn modified_workdir() {}\n");

        let file_b = changed_files.iter().find(|f| f.path == Path::new("file_b.py")).expect("file_b.py should be found");
        assert_eq!(file_b.language, SupportedLanguage::Python);
        assert_eq!(file_b.old_content, "");
        assert_eq!(file_b.new_content, "def staged(): pass\n");

        assert!(changed_files.iter().all(|f| f.path != Path::new("Cargo.lock")));
        assert!(changed_files.iter().all(|f| f.path != Path::new("binary.bin")));
        assert!(changed_files.iter().all(|f| f.path != Path::new("file_c.go")));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_synthetic_git_conflict_simulation() {
        let temp_dir = std::env::temp_dir().join(format!(
            "vibediff_conflict_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let repo = Repository::init(&temp_dir).unwrap();
        let sig = git2::Signature::now("Tester", "tester@example.com").unwrap();

        let file_path = temp_dir.join("conflict.rs");
        std::fs::write(&file_path, "fn compute() -> i32 { 0 }\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("conflict.rs")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let base_commit = repo
            .commit(Some("HEAD"), &sig, &sig, "Base commit", &tree, &[])
            .unwrap();
        drop(tree);
        let base_commit_obj = repo.find_commit(base_commit).unwrap();

        repo.branch("ours", &base_commit_obj, false).unwrap();
        repo.branch("theirs", &base_commit_obj, false).unwrap();

        repo.set_head("refs/heads/ours").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        std::fs::write(&file_path, "fn compute() -> i32 { 10 }\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("conflict.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Ours commit", &tree, &[&base_commit_obj])
            .unwrap();
        drop(tree);

        repo.set_head("refs/heads/theirs").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        std::fs::write(&file_path, "fn compute() -> i32 { 20 }\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("conflict.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let theirs_commit = repo
            .commit(Some("HEAD"), &sig, &sig, "Theirs commit", &tree, &[&base_commit_obj])
            .unwrap();
        drop(tree);
        drop(base_commit_obj);

        repo.set_head("refs/heads/ours").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();

        let theirs_annotated = repo.find_annotated_commit(theirs_commit).unwrap();
        repo.merge(&[&theirs_annotated], None, None).unwrap();

        let extractor = GitDiffExtractor::new(&repo);
        assert!(extractor.has_conflicts().unwrap());

        let conflicts = extractor.get_conflict_changes().unwrap();
        assert_eq!(conflicts.len(), 1);
        let cfile = &conflicts[0];
        assert_eq!(cfile.path, PathBuf::from("conflict.rs"));
        assert_eq!(cfile.language, SupportedLanguage::Rust);
        assert!(cfile.ancestor.is_some());
        assert!(cfile.ours.is_some());
        assert!(cfile.theirs.is_some());
        assert!(cfile.ancestor.as_ref().unwrap().contains("0"));
        assert!(cfile.ours.as_ref().unwrap().contains("10"));
        assert!(cfile.theirs.as_ref().unwrap().contains("20"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_conflict_marker_stripper() {
        let conflict_content = "<<<<<<< HEAD\nfn alpha() -> i32 { 1 }\n=======\nfn alpha() -> i32 { 2 }\n>>>>>>> incoming\n";
        let split = split_conflict_markers(conflict_content);
        assert!(split.is_some());
        let (ours, theirs) = split.unwrap();

        assert_eq!(ours, "fn alpha() -> i32 { 1 }\n");
        assert_eq!(theirs, "fn alpha() -> i32 { 2 }\n");

        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::language()).unwrap();

        let ours_tree = parser.parse(ours.as_bytes(), None).unwrap();
        assert!(!ours_tree.root_node().has_error());

        let theirs_tree = parser.parse(theirs.as_bytes(), None).unwrap();
        assert!(!theirs_tree.root_node().has_error());
    }

    #[test]
    fn test_large_and_deep_untracked_file_rejection() {
        let temp_dir = std::env::temp_dir().join(format!(
            "vibediff_guard_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let repo = Repository::init(&temp_dir).unwrap();

        let large_path = temp_dir.join("large.rs");
        let large_content = vec![b'x'; 300 * 1024];
        std::fs::write(&large_path, large_content).unwrap();

        let deep_dir = temp_dir.join("l1").join("l2").join("l3").join("l4");
        std::fs::create_dir_all(&deep_dir).unwrap();
        let deep_path = deep_dir.join("deep.rs");
        std::fs::write(&deep_path, "fn deep() {}\n").unwrap();

        let normal_path = temp_dir.join("normal.rs");
        std::fs::write(&normal_path, "fn normal() {}\n").unwrap();

        let changed_files = get_changed_files_from_repo(&repo).unwrap();

        assert!(changed_files.iter().any(|f| f.path == Path::new("normal.rs")));
        assert!(changed_files.iter().all(|f| f.path != Path::new("large.rs")));
        assert!(changed_files.iter().all(|f| f.path != Path::new("l1/l2/l3/l4/deep.rs")));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
