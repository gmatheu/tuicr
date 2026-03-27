# Wire save/load dispatch through handler.rs and main.rs

## Request
Connect the in-repo persistence path to the runtime, so that when `App.persistence_mode == Repo`, saving uses `save_session_in_repo()` and loading uses `load_latest_in_repo_session()`. When mode is `Local`, keep existing codepaths. No cross-talk between storage backends.

## Changes

### handler.rs — Save dispatch
- Added `dispatch_save(app)` helper that branches on `app.persistence_mode`:
  - `PersistenceMode::Repo` → calls `save_session_in_repo(&session, &username)` (gets username from `app.vcs.get_current_username()`)
  - `PersistenceMode::Local` → calls `save_session(&session)`
- Replaced direct `save_session()` calls in `:w` and `:wq`/`:x` command handlers with `dispatch_save(app)`

### main.rs — ZZ save dispatch
- Updated ZZ keybinding handler from `if app.in_repo` to `match app.persistence_mode`, using the same Repo/Local branching pattern with VCS username retrieval

### app.rs — Startup load dispatch
- Updated `load_session_for_context()` to accept `vcs: &dyn VcsBackend` parameter
- Split the match on `PersistenceMode`:
  - `Local` → calls `load_latest_session_for_context()` (existing behavior)
  - `Repo` → calls `load_latest_in_repo_session()` with username from VCS and `retention_days=0` (keep forever default)
- Propagated the `vcs` parameter through `load_or_create_session()`, `load_or_create_commit_range_session()`, and `load_or_create_staged_unstaged_and_commits_session()`
- Updated all 10 call sites (5 static in `App::new()`, 5 instance methods) to pass `vcs.as_ref()` / `self.vcs.as_ref()`

### persistence/storage.rs — Fix save/load compatibility
- Fixed `save_session_in_repo()` to use `in_repo_session_filename()` (user-prefixed) instead of `session_filename()` (repo-name-prefixed). The previous implementation used the local filename convention, which was incompatible with `load_latest_in_repo_session()` that filters by `{username}_` prefix.
- Added `username: &str` parameter to `save_session_in_repo()` signature

## Key Design Decisions
- **retention_days defaults to 0** in the load dispatch (keep forever). Config-driven retention can be wired in a future task without changing the dispatch logic.
- **Username fallback**: Both save and load paths use `vcs.get_current_username().unwrap_or("anonymous")` for consistency.
- **No cross-talk**: Local and Repo storage paths are completely independent — local saves go to `~/.local/share/tuicr/reviews/`, repo saves go to `<repo>/.tuicr/reviews/`. Tests verify neither backend finds the other's sessions.

## Tests Added (8 new tests, 330 total passing)
- `persistence_dispatch_tests::load_context_repo_mode_finds_in_repo_session`
- `persistence_dispatch_tests::load_context_repo_mode_ignores_local_sessions`
- `persistence_dispatch_tests::load_context_local_mode_ignores_repo_sessions`
- `no_cross_talk_local_load_ignores_repo_sessions`
- `no_cross_talk_repo_load_ignores_local_sessions`
- `save_session_in_repo_round_trips_with_load`
- `save_session_in_repo_stores_under_tuicr_reviews`
- `save_session_local_does_not_write_to_repo`
