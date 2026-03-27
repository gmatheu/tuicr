use chrono::Utc;
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::error::{Result, TuicrError};
use crate::model::ReviewSession;
use crate::model::review::SessionDiffSource;

const SESSION_MAX_AGE_DAYS: u64 = 7;
const SESSION_FILENAME_MIN_PARTS: usize = 6;
const SESSION_FILENAME_SUFFIX_PARTS: usize = 4;
const SESSION_FILENAME_DATE_LEN: usize = 8;
const SESSION_FILENAME_TIME_LEN: usize = 6;
const FINGERPRINT_HEX_LEN: usize = 8;

struct SessionFilenameParts {
    repo_fingerprints: Vec<String>,
    diff_source: String,
}

fn parse_session_filename(filename: &str) -> Option<SessionFilenameParts> {
    let stem = filename.strip_suffix(".json")?;
    let parts: Vec<&str> = stem.split('_').collect();

    if parts.len() < SESSION_FILENAME_MIN_PARTS {
        return None;
    }

    let diff_source_idx = parts.len().checked_sub(SESSION_FILENAME_SUFFIX_PARTS)?;
    let date_idx = parts.len().checked_sub(SESSION_FILENAME_SUFFIX_PARTS - 1)?;
    let time_idx = parts.len().checked_sub(SESSION_FILENAME_SUFFIX_PARTS - 2)?;
    let diff_source = parts.get(diff_source_idx)?;
    let date_part = parts.get(date_idx)?;
    let time_part = parts.get(time_idx)?;

    if !matches!(
        *diff_source,
        "worktree" | "commits" | "worktree_and_commits"
    ) {
        return None;
    }

    if !is_timestamp_part(date_part, SESSION_FILENAME_DATE_LEN)
        || !is_timestamp_part(time_part, SESSION_FILENAME_TIME_LEN)
    {
        return None;
    }

    let mut fingerprints = Vec::new();
    for part in &parts[..diff_source_idx] {
        if is_hex_fingerprint(part) && !fingerprints.iter().any(|candidate| candidate == part) {
            fingerprints.push((*part).to_string());
        }
    }

    if fingerprints.is_empty() {
        return None;
    }

    Some(SessionFilenameParts {
        repo_fingerprints: fingerprints,
        diff_source: diff_source.to_string(),
    })
}

fn is_timestamp_part(part: &str, len: usize) -> bool {
    part.len() == len && part.chars().all(|ch| ch.is_ascii_digit())
}

fn is_hex_fingerprint(part: &str) -> bool {
    part.len() == FINGERPRINT_HEX_LEN && part.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn get_reviews_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(dir) = std::env::var_os("TUICR_REVIEWS_DIR") {
        let path = PathBuf::from(dir);
        fs::create_dir_all(&path)?;
        return Ok(path);
    }

    let proj_dirs = ProjectDirs::from("", "", "tuicr").ok_or_else(|| {
        TuicrError::Io(std::io::Error::other("Could not determine data directory"))
    })?;

    let data_dir = proj_dirs.data_dir().join("reviews");
    fs::create_dir_all(&data_dir)?;
    Ok(data_dir)
}

const MAX_FILENAME_COMPONENT_LEN: usize = 64;

fn sanitize_filename_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len().min(MAX_FILENAME_COMPONENT_LEN));
    for ch in value.chars() {
        if sanitized.len() >= MAX_FILENAME_COMPONENT_LEN {
            break;
        }
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.');
        sanitized.push(if ok { ch } else { '-' });
    }

    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized.to_string()
    }
}

/// Returns the in-repo reviews directory for a given repository path.
/// The directory is expected to live at <repo_path>/.tuicr/reviews.
/// The function will attempt to create the directory if possible and
/// simply return the path regardless of creation outcome.
pub fn get_in_repo_reviews_dir(repo_path: &Path) -> PathBuf {
    let path = repo_path.join(".tuicr").join("reviews");
    // Best-effort creation; do not fail callers if the filesystem is read-only
    let _ = fs::create_dir_all(&path);
    path
}

/// Generate an in-repo session filename for a given ReviewSession and user.
/// Pattern (approximate): {username}_{base_short}_{head_short}_{timestamp}_{uuid_fragment}.json
/// - username: sanitized to be filesystem-friendly
/// - base_short: first 8 chars of the base commit (or whole value if shorter)
/// - head_short: derived from commit range if present, otherwise a short repo fingerprint
/// - timestamp: UTC timestamp of session creation in YYYYMMDD_HHMMSS
/// - uuid_fragment: first segment of the session UUID (split on '-')
pub fn in_repo_session_filename(session: &ReviewSession, username: &str) -> String {
    // Base short (8 chars max)
    let base_short = session.base_commit.chars().take(8).collect::<String>();

    // Head short: prefer last commit in range if provided, otherwise derive from repo path fingerprint
    let head_short: String = if let Some(range) = &session.commit_range {
        if let Some(last) = range.last() {
            let mut v = sanitize_filename_component(last);
            if v.len() > 8 {
                v.truncate(8);
            }
            v
        } else {
            // No range entries; fallback to a stable fingerprint
            let mut v = repo_path_fingerprint(&session.repo_path);
            if v.len() > 8 {
                v.truncate(8);
            }
            v
        }
    } else {
        let mut v = repo_path_fingerprint(&session.repo_path);
        if v.len() > 8 {
            v.truncate(8);
        }
        v
    };

    // Timestamp
    let timestamp = session.created_at.format("%Y%m%d_%H%M%S").to_string();
    // ID fragment
    let id_fragment = session
        .id
        .split('-')
        .next()
        .unwrap_or(&session.id)
        .to_string();
    // Username sanitized
    let user = sanitize_filename_component(username);

    format!(
        "{}_{}_{}_{}_{}.json",
        user, base_short, head_short, timestamp, id_fragment
    )
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn repo_path_fingerprint(repo_path: &Path) -> String {
    let normalized = normalize_repo_path(repo_path);
    let hash = fnv1a_64(normalized.as_bytes());
    let hex = format!("{hash:016x}");
    hex[..FINGERPRINT_HEX_LEN].to_string()
}

fn normalize_repo_path(repo_path: &Path) -> String {
    let canonical = fs::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
    let normalized = canonical.to_string_lossy().to_string();

    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

fn session_filename(session: &ReviewSession) -> String {
    let repo_name = session
        .repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let repo_name = sanitize_filename_component(repo_name);
    let repo_fingerprint = repo_path_fingerprint(&session.repo_path);

    let branch = session.branch_name.as_deref().unwrap_or("detached");
    let branch = sanitize_filename_component(branch);

    let diff_source = match session.diff_source {
        SessionDiffSource::WorkingTree => "worktree",
        SessionDiffSource::Staged => "staged",
        SessionDiffSource::Unstaged => "unstaged",
        SessionDiffSource::StagedAndUnstaged => "staged_and_unstaged",
        SessionDiffSource::CommitRange => "commits",
        SessionDiffSource::WorkingTreeAndCommits => "worktree_and_commits",
        SessionDiffSource::StagedUnstagedAndCommits => "staged_unstaged_and_commits",
    };

    let timestamp = session.created_at.format("%Y%m%d_%H%M%S");
    let id_fragment = session.id.split('-').next().unwrap_or(&session.id);

    format!(
        "{}_{}_{}_{}_{}_{}.json",
        repo_name, repo_fingerprint, branch, diff_source, timestamp, id_fragment
    )
}

pub fn save_session(session: &ReviewSession) -> Result<PathBuf> {
    let reviews_dir = get_reviews_dir()?;
    let filename = session_filename(session);
    let path = reviews_dir.join(&filename);

    let json = serde_json::to_string_pretty(session)?;
    fs::write(&path, json)?;

    Ok(path)
}

pub fn save_session_in_repo(session: &ReviewSession, username: &str) -> Result<PathBuf> {
    let dir = get_in_repo_reviews_dir(&session.repo_path);
    fs::create_dir_all(&dir)?;
    let filename = in_repo_session_filename(session, username);
    let path = dir.join(&filename);
    let json = serde_json::to_string_pretty(session)?;
    fs::write(&path, json)?;
    Ok(path)
}

/// Loads the most recent in-repo session for `current_user`, filtering by
/// `diff_source` and optional `commit_range`.  Sessions older than
/// `retention_days` are skipped (never deleted); `0` disables the age check.
/// The stored `repo_path` is replaced with the caller-supplied value.
pub fn load_latest_in_repo_session(
    repo_path: &Path,
    diff_source: SessionDiffSource,
    commit_range: Option<&[String]>,
    current_user: &str,
    retention_days: u32,
) -> Result<Option<(PathBuf, ReviewSession)>> {
    let reviews_dir = get_in_repo_reviews_dir(repo_path);

    if !reviews_dir.is_dir() {
        return Ok(None);
    }

    let sanitized_user = sanitize_filename_component(current_user);
    let user_prefix = format!("{sanitized_user}_");

    let now = Utc::now();

    let mut best: Option<(PathBuf, ReviewSession)> = None;

    let entries = fs::read_dir(&reviews_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        if !filename.starts_with(&user_prefix) {
            continue;
        }

        let Ok(mut session) = load_session(&path) else {
            continue;
        };

        if session.diff_source != diff_source {
            continue;
        }

        if matches!(
            diff_source,
            SessionDiffSource::CommitRange
                | SessionDiffSource::WorkingTreeAndCommits
                | SessionDiffSource::StagedUnstagedAndCommits
        ) && let Some(expected_range) = commit_range
            && session.commit_range.as_deref() != Some(expected_range)
        {
            continue;
        }

        if retention_days > 0 {
            let age = now.signed_duration_since(session.updated_at);
            if age.num_days() > i64::from(retention_days) {
                continue;
            }
        }

        session.repo_path = repo_path.to_path_buf();

        let dominated = best
            .as_ref()
            .is_some_and(|(_, existing)| existing.updated_at >= session.updated_at);
        if !dominated {
            best = Some((path, session));
        }
    }

    Ok(best)
}

pub fn load_session(path: &PathBuf) -> Result<ReviewSession> {
    let contents = fs::read_to_string(path)?;
    let session: ReviewSession =
        serde_json::from_str(&contents).map_err(|e| TuicrError::CorruptedSession(e.to_string()))?;
    Ok(session)
}

pub fn load_latest_session_for_context(
    repo_path: &Path,
    branch_name: Option<&str>,
    head_commit: &str,
    diff_source: SessionDiffSource,
    commit_range: Option<&[String]>,
) -> Result<Option<(PathBuf, ReviewSession)>> {
    let current_repo_path = normalize_repo_path(repo_path);
    let current_fingerprint = repo_path_fingerprint(repo_path);
    let current_diff_source = match diff_source {
        SessionDiffSource::WorkingTree => "worktree",
        SessionDiffSource::Staged => "staged",
        SessionDiffSource::Unstaged => "unstaged",
        SessionDiffSource::StagedAndUnstaged => "staged_and_unstaged",
        SessionDiffSource::CommitRange => "commits",
        SessionDiffSource::WorkingTreeAndCommits => "worktree_and_commits",
        SessionDiffSource::StagedUnstagedAndCommits => "staged_unstaged_and_commits",
    };

    let reviews_dir = get_reviews_dir()?;
    let now = SystemTime::now();
    let max_age = Duration::from_secs(SESSION_MAX_AGE_DAYS * 24 * 60 * 60);

    let mut session_files: Vec<_> = fs::read_dir(&reviews_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let path = entry.path();

            if !path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            {
                return false;
            }

            // Delete sessions older than 7 days
            if let Ok(metadata) = entry.metadata()
                && let Ok(modified) = metadata.modified()
                && let Ok(age) = now.duration_since(modified)
                && age > max_age
            {
                let _ = fs::remove_file(&path);
                return false;
            }

            let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
                return false;
            };

            let Some(parts) = parse_session_filename(filename) else {
                return true;
            };

            if !parts
                .repo_fingerprints
                .iter()
                .any(|fingerprint| fingerprint == &current_fingerprint)
            {
                return false;
            }

            if parts.diff_source != current_diff_source {
                return false;
            }

            true
        })
        .collect();

    session_files.sort_by(|a, b| {
        let a_modified = a
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let b_modified = b
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        b_modified
            .cmp(&a_modified)
            .then_with(|| a.file_name().cmp(&b.file_name()))
    });

    let mut legacy_candidate = None;

    for entry in session_files {
        let path = entry.path();
        let Ok(session) = load_session(&path) else {
            continue;
        };

        if normalize_repo_path(&session.repo_path) != current_repo_path {
            continue;
        }

        if session.diff_source != diff_source {
            continue;
        }

        if matches!(
            diff_source,
            SessionDiffSource::CommitRange
                | SessionDiffSource::WorkingTreeAndCommits
                | SessionDiffSource::StagedUnstagedAndCommits
        ) && let Some(expected_range) = commit_range
            && session.commit_range.as_deref() != Some(expected_range)
        {
            continue;
        }

        let session_branch = session.branch_name.as_deref();
        if session_branch == branch_name {
            if branch_name.is_none() && session.base_commit != head_commit {
                continue;
            }

            return Ok(Some((path, session)));
        }

        let eligible_legacy = branch_name.is_some()
            && legacy_candidate.is_none()
            && commit_range.is_none()
            && session_branch.is_none()
            && session.base_commit == head_commit;
        if eligible_legacy {
            legacy_candidate = Some((path, session));
        }
    }

    Ok(legacy_candidate)
}

#[cfg(test)]
fn delete_session(path: &PathBuf) -> Result<()> {
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileStatus;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    const TEST_MTIME_RETRIES: usize = 40;
    const TEST_MTIME_SLEEP_MS: u64 = 100;

    fn create_test_session() -> ReviewSession {
        let mut session = ReviewSession::new(
            PathBuf::from("/tmp/test-repo"),
            "abc1234def".to_string(),
            Some("main".to_string()),
            SessionDiffSource::WorkingTree,
        );
        session.add_file(PathBuf::from("src/main.rs"), FileStatus::Modified);
        session
    }

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct TestReviewsDirGuard<'a> {
        _lock: std::sync::MutexGuard<'a, ()>,
        path: PathBuf,
    }

    impl Drop for TestReviewsDirGuard<'_> {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("TUICR_REVIEWS_DIR");
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn with_test_reviews_dir() -> TestReviewsDirGuard<'static> {
        let lock = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let path =
            std::env::temp_dir().join(format!("tuicr-reviews-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        unsafe {
            std::env::set_var("TUICR_REVIEWS_DIR", path.as_os_str());
        }

        TestReviewsDirGuard { _lock: lock, path }
    }

    fn create_session(
        repo_path: PathBuf,
        base_commit: &str,
        branch_name: Option<&str>,
        diff_source: SessionDiffSource,
        commit_range: Option<Vec<String>>,
    ) -> ReviewSession {
        let mut session = ReviewSession::new(
            repo_path,
            base_commit.to_string(),
            branch_name.map(|s| s.to_string()),
            diff_source,
        );
        session.commit_range = commit_range;
        session.add_file(PathBuf::from("src/main.rs"), FileStatus::Modified);
        session
    }

    fn save_legacy_session(reviews_dir: &Path, session: &ReviewSession) -> PathBuf {
        let mut value = serde_json::to_value(session).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("branch_name");
        obj.remove("diff_source");
        obj.remove("commit_range");
        obj.insert(
            "version".to_string(),
            serde_json::Value::String("1.0".to_string()),
        );

        let id_fragment = session.id.split('-').next().unwrap_or(&session.id);
        let path = reviews_dir.join(format!("legacy_{id_fragment}.json"));
        fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        path
    }

    fn ensure_newer_mtime(newer: &Path, older: &Path) {
        let older_time = fs::metadata(older)
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        for _ in 0..TEST_MTIME_RETRIES {
            let newer_time = fs::metadata(newer)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

            if newer_time > older_time {
                return;
            }

            std::thread::sleep(Duration::from_millis(TEST_MTIME_SLEEP_MS));
            let contents = fs::read_to_string(newer).unwrap();
            fs::write(newer, contents).unwrap();
        }

        let newer_time = fs::metadata(newer)
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        assert!(
            newer_time > older_time,
            "failed to produce newer mtime for {}",
            newer.display()
        );
    }

    #[test]
    fn should_generate_correct_filename() {
        let session = create_test_session();
        let filename = session_filename(&session);
        assert!(filename.starts_with("test-repo_"));
        assert!(filename.contains("_main_worktree_"));
        assert!(filename.ends_with(".json"));
    }

    #[test]
    fn should_generate_filename_for_staged_unstaged() {
        let session = create_session(
            PathBuf::from("/tmp/test-repo"),
            "abc1234def",
            Some("main"),
            SessionDiffSource::StagedAndUnstaged,
            None,
        );
        let filename = session_filename(&session);
        assert!(filename.contains("_staged_and_unstaged_"));
    }

    #[test]
    fn should_roundtrip_session() {
        let _guard = with_test_reviews_dir();
        let session = create_test_session();
        let path = save_session(&session).unwrap();
        let loaded = load_session(&path).unwrap();
        assert_eq!(session.id, loaded.id);
        assert_eq!(session.base_commit, loaded.base_commit);
        assert_eq!(session.branch_name, loaded.branch_name);
        assert_eq!(session.diff_source, loaded.diff_source);
        assert_eq!(session.files.len(), loaded.files.len());
        let _ = delete_session(&path);
    }

    #[test]
    fn should_sanitize_branch_name_in_filename() {
        let session = create_session(
            PathBuf::from("/tmp/test-repo"),
            "abc1234def",
            Some("feature/login"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let filename = session_filename(&session);
        assert!(!filename.contains('/'));
        assert!(filename.contains("feature-login"));
    }

    #[test]
    fn should_resolve_in_repo_reviews_dir_path() {
        let repo_path =
            std::env::temp_dir().join(format!("tuicr-inrepo-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();
        let in_repo = get_in_repo_reviews_dir(&repo_path);
        assert_eq!(in_repo, repo_path.join(".tuicr").join("reviews"));
        let _ = fs::remove_dir_all(&repo_path);
    }

    #[test]
    fn should_generate_in_repo_session_filename_basic() {
        let repo_path = PathBuf::from("/tmp/tuicr-inrepo");
        let mut session = ReviewSession::new(
            repo_path,
            "abcdef12".to_string(),
            Some("main".to_string()),
            SessionDiffSource::WorkingTree,
        );
        session.add_file(PathBuf::from("src/main.rs"), FileStatus::Modified);
        let fname = in_repo_session_filename(&session, "alice");
        assert!(fname.starts_with("alice_"));
        assert!(fname.contains("abcdef12"));
        assert!(fname.ends_with(".json"));
    }

    #[test]
    fn should_select_latest_session_for_branch() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let session1 = create_session(
            repo_path.clone(),
            "commit-1",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let path1 = save_session(&session1).unwrap();

        let session2 = create_session(
            repo_path.clone(),
            "commit-2",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let path2 = save_session(&session2).unwrap();
        ensure_newer_mtime(&path2, &path1);
        let (selected_path, selected) = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "head-does-not-matter-for-branch",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected_path, path2);
        assert_ne!(selected_path, path1);
        assert_eq!(selected.base_commit, "commit-2");
    }

    #[test]
    fn should_match_branch_even_when_head_commit_differs() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let session = create_session(
            repo_path.clone(),
            "old-head",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let _ = save_session(&session).unwrap();
        let loaded = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "new-head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        assert!(loaded.is_some());
    }

    #[test]
    fn should_load_session_with_underscore_branch_name() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let session = create_session(
            repo_path.clone(),
            "head-commit",
            Some("feature/with_underscores"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let _ = save_session(&session).unwrap();
        let loaded = load_latest_session_for_context(
            &repo_path,
            Some("feature/with_underscores"),
            "new-head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        assert!(loaded.is_some());
    }

    #[test]
    fn should_load_session_with_hex_like_branch_segment() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let session = create_session(
            repo_path.clone(),
            "head-commit",
            Some("feature/deadbeef_fix"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let _ = save_session(&session).unwrap();
        let loaded = load_latest_session_for_context(
            &repo_path,
            Some("feature/deadbeef_fix"),
            "new-head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        assert!(loaded.is_some());
    }

    #[test]
    fn should_prefer_branch_match_over_legacy_candidate() {
        let guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let branch_session = create_session(
            repo_path.clone(),
            "branch-base",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let branch_path = save_session(&branch_session).unwrap();

        let legacy_source = create_session(
            repo_path.clone(),
            "head-commit",
            None,
            SessionDiffSource::WorkingTree,
            None,
        );
        let legacy_path = save_legacy_session(&guard.path, &legacy_source);
        let (selected_path, _selected) = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "head-commit",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected_path, branch_path);
        assert_ne!(selected_path, legacy_path);
    }

    #[test]
    fn should_fallback_to_legacy_session_when_no_branch_session_exists() {
        let guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let legacy_source = create_session(
            repo_path.clone(),
            "head-commit",
            None,
            SessionDiffSource::WorkingTree,
            None,
        );
        let legacy_path = save_legacy_session(&guard.path, &legacy_source);
        let (selected_path, selected) = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "head-commit",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected_path, legacy_path);
        assert_eq!(selected.branch_name, None);
        assert_eq!(selected.diff_source, SessionDiffSource::WorkingTree);
    }

    #[test]
    fn should_not_select_legacy_session_when_head_commit_differs() {
        let guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let legacy_source = create_session(
            repo_path.clone(),
            "old-head",
            None,
            SessionDiffSource::WorkingTree,
            None,
        );
        let _legacy_path = save_legacy_session(&guard.path, &legacy_source);
        let loaded = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "new-head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn should_require_commit_match_in_detached_head() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let session = create_session(
            repo_path.clone(),
            "detached-head",
            None,
            SessionDiffSource::WorkingTree,
            None,
        );
        let _ = save_session(&session).unwrap();
        let mismatch = load_latest_session_for_context(
            &repo_path,
            None,
            "different-head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        let match_ = load_latest_session_for_context(
            &repo_path,
            None,
            "detached-head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        assert!(mismatch.is_none());
        assert!(match_.is_some());
    }

    #[test]
    fn should_ignore_sessions_with_different_diff_source() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let commit_range = vec!["commit-2".to_string(), "commit-1".to_string()];
        let commits_session = create_session(
            repo_path.clone(),
            "commit-2",
            Some("main"),
            SessionDiffSource::CommitRange,
            Some(commit_range.clone()),
        );
        let _ = save_session(&commits_session).unwrap();
        let worktree = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        let commits = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "head",
            SessionDiffSource::CommitRange,
            Some(commit_range.as_slice()),
        )
        .unwrap();
        assert!(worktree.is_none());
        assert!(commits.is_some());
    }

    #[test]
    fn should_match_commit_range_session() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let commit_range_a = vec!["commit-a2".to_string(), "commit-a1".to_string()];
        let commit_range_b = vec!["commit-b2".to_string(), "commit-b1".to_string()];

        let session_a = create_session(
            repo_path.clone(),
            "commit-a2",
            Some("main"),
            SessionDiffSource::CommitRange,
            Some(commit_range_a.clone()),
        );
        let path_a = save_session(&session_a).unwrap();

        let session_b = create_session(
            repo_path.clone(),
            "commit-b2",
            Some("main"),
            SessionDiffSource::CommitRange,
            Some(commit_range_b.clone()),
        );
        let path_b = save_session(&session_b).unwrap();
        let (selected_path, selected) = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "commit-b2",
            SessionDiffSource::CommitRange,
            Some(commit_range_b.as_slice()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected_path, path_b);
        assert_ne!(selected_path, path_a);
        assert_eq!(
            selected.commit_range.as_deref(),
            Some(commit_range_b.as_slice())
        );
    }

    #[test]
    fn should_roundtrip_commit_range_session() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let commit_range = vec!["commit-2".to_string(), "commit-1".to_string()];
        let session = create_session(
            repo_path,
            "commit-2",
            Some("main"),
            SessionDiffSource::CommitRange,
            Some(commit_range.clone()),
        );
        let path = save_session(&session).unwrap();
        let loaded = load_session(&path).unwrap();
        assert_eq!(loaded.commit_range, Some(commit_range));
        assert_eq!(loaded.diff_source, SessionDiffSource::CommitRange);
        let _ = delete_session(&path);
    }

    #[test]
    fn should_require_commit_range_order_match() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let commit_range = vec!["commit-2".to_string(), "commit-1".to_string()];
        let reversed_range = vec!["commit-1".to_string(), "commit-2".to_string()];

        let session = create_session(
            repo_path.clone(),
            "commit-2",
            Some("main"),
            SessionDiffSource::CommitRange,
            Some(commit_range),
        );
        let _ = save_session(&session).unwrap();
        let loaded = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "commit-2",
            SessionDiffSource::CommitRange,
            Some(reversed_range.as_slice()),
        )
        .unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn should_skip_commit_sessions_without_range_match() {
        let _guard = with_test_reviews_dir();
        let repo_path = std::env::temp_dir().join(format!("tuicr-repo-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&repo_path).unwrap();

        let commit_range = vec!["commit-2".to_string(), "commit-1".to_string()];

        let session = create_session(
            repo_path.clone(),
            "commit-2",
            Some("main"),
            SessionDiffSource::CommitRange,
            None,
        );
        let _ = save_session(&session).unwrap();
        let loaded = load_latest_session_for_context(
            &repo_path,
            Some("main"),
            "commit-2",
            SessionDiffSource::CommitRange,
            Some(commit_range.as_slice()),
        )
        .unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn should_disambiguate_repos_with_same_folder_name() {
        let _guard = with_test_reviews_dir();
        let base = std::env::temp_dir().join(format!("tuicr-repos-{}", uuid::Uuid::new_v4()));
        let repo_a = base.join("a").join("same-repo");
        let repo_b = base.join("b").join("same-repo");
        fs::create_dir_all(&repo_a).unwrap();
        fs::create_dir_all(&repo_b).unwrap();

        let session_a = create_session(
            repo_a.clone(),
            "head-a",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let _ = save_session(&session_a).unwrap();

        let session_b = create_session(
            repo_b.clone(),
            "head-b",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        let _ = save_session(&session_b).unwrap();
        let (_path, selected) = load_latest_session_for_context(
            &repo_a,
            Some("main"),
            "head",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.base_commit, "head-a");
        assert_eq!(
            normalize_repo_path(&selected.repo_path),
            normalize_repo_path(&repo_a)
        );
    }

    fn temp_repo_path() -> PathBuf {
        std::env::temp_dir().join(format!("tuicr-inrepo-test-{}", uuid::Uuid::new_v4()))
    }

    fn save_in_repo_session_file(
        repo_path: &Path,
        session: &ReviewSession,
        username: &str,
    ) -> PathBuf {
        let dir = get_in_repo_reviews_dir(repo_path);
        fs::create_dir_all(&dir).unwrap();
        let filename = in_repo_session_filename(session, username);
        let path = dir.join(filename);
        let json = serde_json::to_string_pretty(session).unwrap();
        fs::write(&path, json).unwrap();
        path
    }

    fn create_aged_session(
        repo_path: PathBuf,
        base_commit: &str,
        branch_name: Option<&str>,
        diff_source: SessionDiffSource,
        commit_range: Option<Vec<String>>,
        days_ago: i64,
    ) -> ReviewSession {
        let mut session = create_session(repo_path, base_commit, branch_name, diff_source, commit_range);
        let past = Utc::now() - chrono::Duration::days(days_ago);
        session.created_at = past;
        session.updated_at = past;
        session
    }

    #[test]
    fn in_repo_load_returns_current_user_session_only() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let alice_session = create_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        save_in_repo_session_file(&repo, &alice_session, "alice");

        let bob_session = create_session(
            repo.clone(),
            "def456",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        save_in_repo_session_file(&repo, &bob_session, "bob");

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(result.is_some());
        let (_, loaded) = result.unwrap();
        assert_eq!(loaded.id, alice_session.id);

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_skips_old_sessions_without_deleting() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let old_session = create_aged_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
            30,
        );
        let old_path = save_in_repo_session_file(&repo, &old_session, "alice");

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            7,
        )
        .unwrap();
        assert!(result.is_none());
        assert!(old_path.exists(), "file must NOT be deleted by retention");

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_zero_retention_loads_all() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let ancient_session = create_aged_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
            365,
        );
        save_in_repo_session_file(&repo, &ancient_session, "alice");

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(result.is_some());
        let (_, loaded) = result.unwrap();
        assert_eq!(loaded.id, ancient_session.id);

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_returns_none_when_no_match() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let session = create_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        save_in_repo_session_file(&repo, &session, "bob");

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(result.is_none());

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_filters_by_diff_source() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let worktree_session = create_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        save_in_repo_session_file(&repo, &worktree_session, "alice");

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::CommitRange,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(result.is_none());

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(result.is_some());

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_matches_commit_range() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let range = vec!["commit-a".to_string(), "commit-b".to_string()];
        let session = create_session(
            repo.clone(),
            "commit-a",
            Some("main"),
            SessionDiffSource::CommitRange,
            Some(range.clone()),
        );
        save_in_repo_session_file(&repo, &session, "alice");

        let wrong_range = vec!["other-a".to_string(), "other-b".to_string()];
        let miss = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::CommitRange,
            Some(wrong_range.as_slice()),
            "alice",
            0,
        )
        .unwrap();
        assert!(miss.is_none());

        let hit = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::CommitRange,
            Some(range.as_slice()),
            "alice",
            0,
        )
        .unwrap();
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().1.id, session.id);

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_returns_most_recent_session() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let older = create_aged_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
            2,
        );
        save_in_repo_session_file(&repo, &older, "alice");

        let newer = create_session(
            repo.clone(),
            "def456",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        save_in_repo_session_file(&repo, &newer, "alice");

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.1.id, newer.id);

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_sanitizes_repo_path() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let mut session = create_session(
            PathBuf::from("/some/other/absolute/path"),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        session.add_file(PathBuf::from("src/main.rs"), FileStatus::Modified);
        save_in_repo_session_file(&repo, &session, "alice");

        let (_, loaded) = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap()
        .unwrap();
        assert_eq!(loaded.repo_path, repo);

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_returns_none_for_empty_dir() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(result.is_none());

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn in_repo_load_returns_none_when_dir_missing() {
        let repo = temp_repo_path();
        let result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn no_cross_talk_local_load_ignores_repo_sessions() {
        let _guard = with_test_reviews_dir();
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let session = create_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        save_in_repo_session_file(&repo, &session, "alice");

        let local = load_latest_session_for_context(
            &repo,
            Some("main"),
            "abc123",
            SessionDiffSource::WorkingTree,
            None,
        )
        .unwrap();
        assert!(
            local.is_none(),
            "local load must not find in-repo sessions"
        );

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn no_cross_talk_repo_load_ignores_local_sessions() {
        let _guard = with_test_reviews_dir();
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let session = create_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        save_session(&session).unwrap();

        let repo_result = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(
            repo_result.is_none(),
            "repo load must not find local sessions"
        );

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn save_session_in_repo_round_trips_with_load() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let mut session = create_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );
        session.add_file(PathBuf::from("src/lib.rs"), FileStatus::Modified);

        let saved_path = save_session_in_repo(&session, "alice").unwrap();
        assert!(saved_path.starts_with(repo.join(".tuicr").join("reviews")));
        assert!(saved_path.exists());

        let loaded = load_latest_in_repo_session(
            &repo,
            SessionDiffSource::WorkingTree,
            None,
            "alice",
            0,
        )
        .unwrap();
        assert!(loaded.is_some(), "should load the session we just saved");
        let (_, loaded_session) = loaded.unwrap();
        assert_eq!(loaded_session.id, session.id);

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn save_session_in_repo_stores_under_tuicr_reviews() {
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let session = create_session(
            repo.clone(),
            "def456",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );

        let path = save_session_in_repo(&session, "bob").unwrap();
        let expected_dir = repo.join(".tuicr").join("reviews");
        assert!(
            path.starts_with(&expected_dir),
            "repo save must write under .tuicr/reviews/"
        );
        assert!(path.exists());

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn save_session_local_does_not_write_to_repo() {
        let _guard = with_test_reviews_dir();
        let repo = temp_repo_path();
        fs::create_dir_all(&repo).unwrap();

        let session = create_session(
            repo.clone(),
            "abc123",
            Some("main"),
            SessionDiffSource::WorkingTree,
            None,
        );

        let path = save_session(&session).unwrap();
        assert!(
            !path.starts_with(&repo),
            "local save must not write under the repo"
        );

        let _ = fs::remove_dir_all(&repo);
    }
}
