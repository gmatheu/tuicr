pub mod context;
pub mod diff;
pub mod repository;

use git2::{ConfigLevel, Repository};
use std::path::Path;

use crate::error::{Result, TuicrError};
use crate::model::{DiffFile, DiffLine, FileStatus};
use crate::syntax::SyntaxHighlighter;

use super::traits::{CommitInfo, VcsBackend, VcsInfo, VcsType};

// Re-export commonly used functions
pub use context::{calculate_gap, fetch_context_lines};
pub use diff::{
    get_commit_range_diff, get_staged_diff, get_unstaged_diff, get_working_tree_diff,
    get_working_tree_with_commits_diff,
};

/// Git backend implementation using git2 library
pub struct GitBackend {
    repo: Repository,
    info: VcsInfo,
}

impl GitBackend {
    /// Discover a git repository from the current directory
    pub fn discover() -> Result<Self> {
        let cwd = std::env::current_dir().map_err(|_| TuicrError::NotARepository)?;
        let repo = Repository::discover(&cwd).map_err(|_| TuicrError::NotARepository)?;

        let root_path = repo
            .workdir()
            .ok_or(TuicrError::NotARepository)?
            .to_path_buf();

        let head_commit = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .map(|c| c.id().to_string())
            .unwrap_or_else(|| "HEAD".to_string());

        let branch_name = repo.head().ok().and_then(|h| {
            if h.is_branch() {
                h.shorthand().map(|s| s.to_string())
            } else {
                None
            }
        });

        let info = VcsInfo {
            root_path,
            head_commit,
            branch_name,
            vcs_type: VcsType::Git,
        };

        Ok(Self { repo, info })
    }
}

#[cfg(test)]
mod username_tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_backend(name: Option<&str>) -> (TempDir, GitBackend) {
        let dir = TempDir::new().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        if let Some(username) = name {
            let config = repo.config().unwrap();
            let mut local = config.open_level(ConfigLevel::Local).unwrap();
            local.set_str("user.name", username).unwrap();
        }

        let backend = GitBackend {
            info: VcsInfo {
                root_path: repo.workdir().unwrap().to_path_buf(),
                head_commit: "HEAD".to_string(),
                branch_name: None,
                vcs_type: VcsType::Git,
            },
            repo,
        };

        (dir, backend)
    }

    #[test]
    fn get_current_username_reads_config_when_present() {
        let (_dir, backend) = setup_backend(Some("Alice"));
        let username = backend
            .get_current_username()
            .expect("Should read username");
        assert_eq!(username, "Alice");
    }

    #[test]
    fn get_current_username_falls_back_to_anonymous_when_missing() {
        let (_dir, backend) = setup_backend(None);
        let username = backend
            .get_current_username()
            .expect("Should be able to read username (fallback)");
        assert_eq!(username, "anonymous");
    }
}

impl VcsBackend for GitBackend {
    fn info(&self) -> &VcsInfo {
        &self.info
    }

    fn get_current_username(&self) -> Result<String> {
        match self
            .repo
            .config()
            .and_then(|cfg| cfg.open_level(ConfigLevel::Local))
        {
            Ok(cfg) => match cfg.get_string("user.name") {
                Ok(name) if !name.trim().is_empty() => Ok(name),
                _ => Ok("anonymous".to_string()),
            },
            Err(_) => Ok("anonymous".to_string()),
        }
    }

    fn get_working_tree_diff(&self, highlighter: &SyntaxHighlighter) -> Result<Vec<DiffFile>> {
        get_working_tree_diff(&self.repo, highlighter)
    }

    fn get_staged_diff(&self, highlighter: &SyntaxHighlighter) -> Result<Vec<DiffFile>> {
        get_staged_diff(&self.repo, highlighter)
    }

    fn get_unstaged_diff(&self, highlighter: &SyntaxHighlighter) -> Result<Vec<DiffFile>> {
        get_unstaged_diff(&self.repo, highlighter)
    }

    fn fetch_context_lines(
        &self,
        file_path: &Path,
        file_status: FileStatus,
        start_line: u32,
        end_line: u32,
    ) -> Result<Vec<DiffLine>> {
        fetch_context_lines(&self.repo, file_path, file_status, start_line, end_line)
    }

    fn get_recent_commits(&self, offset: usize, limit: usize) -> Result<Vec<CommitInfo>> {
        let git_commits = repository::get_recent_commits(&self.repo, offset, limit)?;
        Ok(git_commits
            .into_iter()
            .map(|c| CommitInfo {
                id: c.id,
                short_id: c.short_id,
                branch_name: c.branch_name,
                summary: c.summary,
                body: c.body,
                author: c.author,
                time: c.time,
            })
            .collect())
    }

    fn resolve_revisions(&self, revisions: &str) -> Result<Vec<String>> {
        repository::resolve_revisions(&self.repo, revisions)
    }

    fn get_commit_range_diff(
        &self,
        commit_ids: &[String],
        highlighter: &SyntaxHighlighter,
    ) -> Result<Vec<DiffFile>> {
        get_commit_range_diff(&self.repo, commit_ids, highlighter)
    }

    fn get_commits_info(&self, ids: &[String]) -> Result<Vec<CommitInfo>> {
        let git_commits = repository::get_commits_info(&self.repo, ids)?;
        Ok(git_commits
            .into_iter()
            .map(|c| CommitInfo {
                id: c.id,
                short_id: c.short_id,
                branch_name: c.branch_name,
                summary: c.summary,
                body: c.body,
                author: c.author,
                time: c.time,
            })
            .collect())
    }

    fn get_working_tree_with_commits_diff(
        &self,
        commit_ids: &[String],
        highlighter: &SyntaxHighlighter,
    ) -> Result<Vec<DiffFile>> {
        get_working_tree_with_commits_diff(&self.repo, commit_ids, highlighter)
    }
}
