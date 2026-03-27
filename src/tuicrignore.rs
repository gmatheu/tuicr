use std::path::Path;

use ignore::gitignore::GitignoreBuilder;

use crate::model::DiffFile;

/// Apply `.tuicrignore` rules from the repository root to a diff file set.
pub fn filter_diff_files(repo_root: &Path, diff_files: Vec<DiffFile>) -> Vec<DiffFile> {
    // Always exclude any files under the ".tuicr" directory from diffs.
    // This prevents leakage of repository-internal review artifacts.
    // Additionally, respect .tuicrignore patterns if they exist.
    let Some(matcher) = load_matcher(repo_root) else {
        return diff_files
            .into_iter()
            .filter(|file| {
                let path = file.display_path();
                // Exclude root- and nested .tuicr directory occurrences
                !path.components().any(|c| c.as_os_str() == ".tuicr")
            })
            .collect();
    };

    diff_files
        .into_iter()
        .filter(|file| {
            let path = file.display_path();
            // Exclude any path that is under .tuicr/
            if path.components().any(|c| c.as_os_str() == ".tuicr") {
                return false;
            }
            !matcher.matched_path_or_any_parents(path, false).is_ignore()
        })
        .collect()
}

fn load_matcher(repo_root: &Path) -> Option<ignore::gitignore::Gitignore> {
    let ignore_file = repo_root.join(".tuicrignore");
    if !ignore_file.is_file() {
        return None;
    }

    let mut builder = GitignoreBuilder::new(repo_root);
    // Ignore malformed patterns and continue with valid ones.
    let _ = builder.add(ignore_file);
    builder.build().ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::*;
    use crate::model::FileStatus;

    fn make_diff_file(path: &str) -> DiffFile {
        DiffFile {
            old_path: None,
            new_path: Some(PathBuf::from(path)),
            status: FileStatus::Modified,
            hunks: Vec::new(),
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
        }
    }

    #[test]
    fn keeps_all_files_when_tuicrignore_is_missing() {
        let dir = tempdir().expect("failed to create temp dir");
        let files = vec![
            make_diff_file("src/main.rs"),
            make_diff_file("target/debug/app"),
        ];

        let filtered = filter_diff_files(dir.path(), files);

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filters_matching_files() {
        let dir = tempdir().expect("failed to create temp dir");
        let ignore_path = dir.path().join(".tuicrignore");
        fs::write(&ignore_path, "target/\n*.lock\n").expect("failed to write .tuicrignore");

        let files = vec![
            make_diff_file("src/main.rs"),
            make_diff_file("target/debug/app"),
            make_diff_file("Cargo.lock"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["src/main.rs"]);
    }

    #[test]
    fn supports_unignore_rules() {
        let dir = tempdir().expect("failed to create temp dir");
        let ignore_path = dir.path().join(".tuicrignore");
        fs::write(&ignore_path, "generated/\n!generated/keep.rs\n")
            .expect("failed to write .tuicrignore");

        let files = vec![
            make_diff_file("generated/drop.rs"),
            make_diff_file("generated/keep.rs"),
            make_diff_file("src/main.rs"),
        ];

        let filtered = filter_diff_files(dir.path(), files);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["generated/keep.rs", "src/main.rs"]);
    }

    #[test]
    fn handles_deleted_file_paths() {
        let dir = tempdir().expect("failed to create temp dir");
        let ignore_path = dir.path().join(".tuicrignore");
        fs::write(&ignore_path, "generated/\n").expect("failed to write .tuicrignore");

        let deleted = DiffFile {
            old_path: Some(PathBuf::from("generated/old.txt")),
            new_path: None,
            status: FileStatus::Deleted,
            hunks: Vec::new(),
            is_binary: false,
            is_too_large: false,
            is_commit_message: false,
        };
        let kept = make_diff_file("src/lib.rs");

        let filtered = filter_diff_files(dir.path(), vec![deleted, kept]);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["src/lib.rs"]);
    }

    #[test]
    fn excludes_root_and_nested_tuicr_paths() {
        let dir = tempdir().expect("failed to create temp dir");
        // Root-level .tuicr path
        let f_root = make_diff_file(".tuicr/reviews/notes.txt");
        // Nested .tuicr path
        let f_nested = make_diff_file("src/.tuicr/hidden.rs");
        // A normal file should be kept
        let f_ok = make_diff_file("src/main.rs");

        let filtered = filter_diff_files(dir.path(), vec![f_root, f_nested, f_ok]);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["src/main.rs"]);
    }

    #[test]
    fn excludes_tuicr_paths_even_with_ignore_file() {
        let dir = tempdir().expect("failed to create temp dir");
        let ignore_path = dir.path().join(".tuicrignore");
        // A generic ignore pattern that would normally filter nothing in this test
        fs::write(&ignore_path, "target/\n").expect("failed to write .tuicrignore");

        // Tuicr path should be excluded regardless of ignore rules
        let f_root = make_diff_file(".tuicr/reviews/notes.txt");
        let f_ok = make_diff_file("src/main.rs");

        let filtered = filter_diff_files(dir.path(), vec![f_root, f_ok]);
        let kept_paths: Vec<String> = filtered
            .iter()
            .map(|f| f.display_path().display().to_string())
            .collect();

        assert_eq!(kept_paths, vec!["src/main.rs"]);
    }
}
