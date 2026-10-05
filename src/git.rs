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

pub fn get_changed_files_from_repo(repo: &Repository) -> Result<Vec<ChangedFile>> {
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

        let new_content = match String::from_utf8(new_bytes) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let old_content = match &head_tree {
            Some(tree) => {
                let lookup_path = entry
                    .head_to_index()
                    .and_then(|diff| diff.old_file().path().and_then(|p| p.to_str()))
                    .unwrap_or(path_str);
                let posix_path = lookup_path.replace('\\', "/");
                match tree.get_path(Path::new(&posix_path)) {
                    Ok(tree_entry) => {
                        let object = tree_entry.to_object(repo).with_context(|| {
                            format!("Failed to retrieve tree object for {:?}", posix_path)
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
}
