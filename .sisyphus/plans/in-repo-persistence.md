# In-Repo Persistence for tuicr

## TL;DR

> **Quick Summary**: Add optional in-repository persistence mode for tuicr reviews, saving to `.tuicr/reviews/` instead of local storage. Enables sharing reviews via git and cross-machine access.
> 
> **Deliverables**:
> - Config option `persistence = "repo"` with CLI flag `--in-repo`
> - In-repo storage at `.tuicr/reviews/{username}_{base}_{head}_{timestamp}_{uuid}.{json,md}`
> - Automatic dual-file export (JSON + Markdown) on every save
> - Per-user filenames using git-configured username with fallback
> - Configurable retention with no auto-delete for in-repo files
> - Absolute path sanitization in saved sessions
> 
> **Estimated Effort**: Medium (10 waves, ~12 tasks)
> **Parallel Execution**: YES - 3 waves of parallel tasks, then sequential integration
> **Critical Path**: Task 1 → Task 3 → Task 7 → Task 9 → Final Verification

---

## Context

### Original Request
Enable saving tuicr code reviews directly into the repository instead of local user storage (`~/.local/share/tuicr/reviews/`), making reviews shareable via git and accessible across different machines.

### Interview Summary

**Key Design Decisions**:
1. Storage: `.tuicr/reviews/` directory at repo root (hidden, consistent with existing `.tuicrignore` pattern)
2. Filename: `{username}_{base}_{head}_{timestamp}_{uuid}.{json,md}` - commit-range based, per-user via git username
3. Git integration: Minimal - just write files, user manually commits if desired
4. Default: Local storage remains default, opt-in via config (`persistence = "repo"`) or CLI flag (`--in-repo`)
5. Migration: New reviews only, existing local reviews stay local (no migration)
6. Markdown export: Always save both `.json` and `.md` on every save
7. Retention: Configurable per-repo via `in_repo_retention_days` (0 = keep forever, default)
8. Test strategy: TDD (Test-First) - RED-GREEN-REFACTOR workflow

### Research Findings

**Current Persistence Layer**:
- Local storage: `~/.local/share/tuicr/reviews/`
- Filename: `{repo_name}_{fingerprint}_{branch}_{diff_source}_{timestamp}_{id}.json`
- Repo binding: Uses 8-char hex fingerprint of canonicalized repo path (breaks across machines)
- `ReviewSession.repo_path`: Stores absolute local path (e.g., `/home/user/project`) - **PRIVACY LEAK**
- Loading: Scans directory, filters by fingerprint + diff_source + branch, 7-day auto-delete
- JSON schema: ReviewSession with id, version, repo_path, branch_name, base_commit, diff_source, commit_range, timestamps, files, comments

### Metis Review

**Critical Gaps Addressed**:
1. **ReviewSession.repo_path leakage**: Absolute paths in saved JSON leak local filesystem layout and break cross-machine loading. **Resolution**: Sanitize to relative path or store empty for in-repo mode.

2. **Username retrieval missing**: No VcsBackend method to get current user's configured name. **Resolution**: Add `get_current_username()` to trait and all implementations.

3. **Markdown export needs runtime state**: `generate_export_content()` requires `&DiffSource` not available in `save_session()`. **Resolution**: Add new save function signature or pass DiffSource through.

4. **Retention auto-deletes git-tracked files**: Current logic deletes old sessions. **Resolution**: For in-repo, only skip loading (don't delete files).

5. **`.tuicr/` files appear in diff view**: Saved review files would display in tuicr's own diff. **Resolution**: Auto-filter `.tuicr/` in `tuicrignore.rs` regardless of user config.

6. **Filename length limits**: Full commit hashes (40 chars) could exceed filesystem limits. **Resolution**: Use short hashes (7-8 chars) in filenames.

**Guardrails Applied**:
- `save_session()` signature must NOT change (3 existing call sites in handler.rs and main.rs)
- No migration of existing local sessions
- No git add/commit automation
- No multi-user session merging
- No `.tuicr/` directory initialization wizard
- `KNOWN_KEYS` in config must include new fields atomically

---

## Work Objectives

### Core Objective
Add in-repository persistence mode to tuicr, enabling review sessions to be saved at `.tuicr/reviews/` as shareable files while maintaining backward compatibility with existing local storage.

### Concrete Deliverables
- `src/config/mod.rs`: Add `persistence: String` and `in_repo_retention_days: u32` config options
- `src/vcs/traits.rs`: Add `get_current_username(&self) -> Result<String>` to `VcsBackend` trait
- `src/vcs/git/mod.rs`, `hg/mod.rs`, `jj/mod.rs`: Implement username retrieval with fallback
- `src/persistence/storage.rs`: Add in-repo storage functions (path resolution, filename generation, save/load with retention)
- `src/persistence/mod.rs`: Export new in-repo functions
- `src/tuicrignore.rs`: Auto-filter `.tuicr/` paths
- `src/theme/mod.rs`: Add `--in-repo` CLI flag to `CliArgs`
- `src/main.rs`: Wire CLI flag and config resolution
- `src/app.rs`: Integrate persistence mode selection
- `src/handler.rs`: Dispatch to correct save location based on mode

### Definition of Done
- [ ] `cargo test` passes all new and existing tests
- [ ] In-repo save creates both `.json` and `.md` files
- [ ] In-repo load finds current user's sessions only
- [ ] `repo_path` in in-repo JSON is relative (not absolute)
- [ ] `.tuicr/` files don't appear in tuicr's diff view
- [ ] Retention skips loading old files but doesn't delete them
- [ ] CLI `--in-repo` overrides config `persistence = "local"`
- [ ] Username fallback works when git config unset

### Must Have
- Config-based persistence mode selection (`local` | `repo`)
- CLI flag `--in-repo` to override config
- In-repo storage at `.tuicr/reviews/`
- Per-user filenames with git-username
- Automatic JSON + Markdown export on save
- Configurable retention (default: keep forever)
- Absolute path sanitization for in-repo sessions
- TDD: All new code has tests first

### Must NOT Have (Guardrails)
- No migration of existing local sessions to in-repo
- No git add/commit automation
- No multi-user session merging or conflict resolution
- No `.tuicr/` directory initialization prompts
- No backward-incompatible changes to `ReviewSession` schema
- No auto-deletion of in-repo files (retention only filters, doesn't delete)
- No modification of `save_session()` public signature

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (bun test framework, existing test patterns)
- **Automated tests**: TDD (RED-GREEN-REFACTOR)
- **Framework**: `cargo test` (Rust built-in test framework)
- **TDD workflow**: Each task starts with failing test(s), then minimal implementation to pass, then refactor

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Backend/Library**: Use `cargo test` with specific test names
- **Config/CLI**: Use Bash to verify config parsing and CLI flag behavior
- **Integration**: Use Bash with temporary git repo setup

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation - Can All Run in Parallel):
├── Task 1: Add get_current_username() to VcsBackend trait + git impl
├── Task 2: Add hg username retrieval implementation
├── Task 3: Add jj username retrieval implementation
└── Task 4: Add config options (persistence, in_repo_retention_days)

Wave 2 (CLI & Path Resolution - Can Run After Wave 1):
├── Task 5: Add --in-repo CLI flag to CliArgs
├── Task 6: Add in-repo storage path resolution + filename generation
└── Task 7: Wire config + CLI through App persistence mode resolution

Wave 3 (Save/Load Implementation - Depends on Waves 1-2):
├── Task 8: Implement in-repo save (JSON + MD dual export)
└── Task 9: Implement in-repo load with user-scoping + no-delete retention

Wave 4 (Integration & Filtering - Sequential After Wave 3):
├── Task 10: Auto-filter .tuicr/ from diff view in tuicrignore.rs
└── Task 11: Wire save/load dispatch through handler.rs and main.rs

Wave FINAL (Verification - 4 Parallel Reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (cargo test, cargo clippy)
├── Task F3: Real integration QA (unspecified-high)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay
```

### Dependency Matrix

| Task | Blocks | Blocked By |
|------|--------|--------------|
| 1 | 2, 3 | None (can start immediately) |
| 2 | - | 1 |
| 3 | - | 1 |
| 4 | 7 | None |
| 5 | 7 | None |
| 6 | 8 | 4 |
| 7 | 8, 9 | 1, 4, 5 |
| 8 | 11 | 6, 7 |
| 9 | 11 | 6, 7 |
| 10 | 11 | None (independent) |
| 11 | F1-F4 | 8, 9, 10 |
| F1-F4 | - | 11 |

**Critical Path**: Task 1 → Task 3 → Task 7 → Task 9 → Task 11 → Final Verification
**Parallel Speedup**: ~60% faster than sequential
**Max Concurrent**: 4 (Wave 1)

### Agent Dispatch Summary

- **Wave 1**: **4 tasks** → T1: `quick`, T2: `quick`, T3: `quick`, T4: `quick`
- **Wave 2**: **3 tasks** → T5: `quick`, T6: `quick`, T7: `quick`
- **Wave 3**: **2 tasks** → T8: `unspecified-high`, T9: `unspecified-high`
- **Wave 4**: **2 tasks** → T10: `quick`, T11: `unspecified-high`
- **FINAL**: **4 tasks** → F1: `oracle`, F2: `unspecified-high`, F3: `unspecified-high`, F4: `deep`

---

## TODOs

- [x] 1. Add get_current_username() to VcsBackend trait + git implementation

  **What to do**:
  - Add `get_current_username(&self) -> Result<String>` method to `VcsBackend` trait in `src/vcs/traits.rs`
  - Implement for `GitBackend` in `src/vcs/git/mod.rs` using `git2::Config::open_default()?.get_string("user.name")`
  - Handle error cases: config not found → return `"anonymous"`, other errors → propagate
  - Write unit tests in `src/vcs/git/mod.rs` testing: success case, missing config, error propagation

  **Must NOT do**:
  - Don't implement for hg or jj backends (separate tasks)
  - Don't use `unwrap()` - always return `Result`
  - Don't change existing trait methods

  **Recommended Agent Profile**:
  - **Category**: `quick` (focused trait addition)
  - **Skills**: []
  - **Skills Evaluated but Omitted**: `git` (using git2 crate directly, not external git skill)

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 1 (with Tasks 2, 3, 4)
  - **Parallel Group**: Wave 1
  - **Blocks**: Task 2, Task 3
  - **Blocked By**: None

  **References**:
  - `src/vcs/traits.rs` - VcsBackend trait definition
  - `src/vcs/git/mod.rs` - GitBackend implementation
  - `src/vcs/git/repository.rs` - Example of git2 usage (see `get_recent_commits()` for config pattern)
  - `anyhow::Result` error handling pattern used throughout codebase

  **Acceptance Criteria**:

  **RED Phase (Write failing tests first)**:
  - [ ] Test: `get_current_username()` returns git user.name when configured
    ```rust
    // Mock with temp git repo having user.name = "Test User"
    let backend = GitBackend::new(&repo_path)?;
    assert_eq!(backend.get_current_username()?, "Test User");
    ```
  - [ ] Test: Returns `"anonymous"` when user.name not set
    ```rust
    // Mock with temp git repo without user.name configured
    assert_eq!(backend.get_current_username()?, "anonymous");
    ```

  **GREEN Phase (Minimal implementation)**:
  - [ ] Add `get_current_username(&self) -> Result<String>` to trait
  - [ ] Implement git version using git2 config

  **QA Scenarios**:

  ```
  Scenario: Git username retrieval success
    Tool: cargo test
    Preconditions: Git repo with `user.name = "Alice Developer"` configured
    Steps:
      1. Create temp dir, init git repo
      2. Run `git config user.name "Alice Developer"`
      3. Call `GitBackend::new(&path)?.get_current_username()`
    Expected Result: Returns `Ok("Alice Developer")`
    Evidence: .sisyphus/evidence/task-1-username-success.txt

  Scenario: Git username fallback when not configured
    Tool: cargo test
    Preconditions: Git repo WITHOUT user.name configured
    Steps:
      1. Create temp dir, init git repo (no user.name)
      2. Call `GitBackend::new(&path)?.get_current_username()`
    Expected Result: Returns `Ok("anonymous")` (graceful fallback)
    Evidence: .sisyphus/evidence/task-1-username-fallback.txt
  ```

  **Evidence to Capture**:
  - [ ] `task-1-username-success.txt`: Test output showing successful username retrieval
  - [ ] `task-1-username-fallback.txt`: Test output showing fallback to "anonymous"

  **Commit**: YES (Commit 1)
  - Message: `feat(vcs): add get_current_username() to VcsBackend trait with git impl`
  - Files: `src/vcs/traits.rs`, `src/vcs/git/mod.rs`
  - Pre-commit: `cargo test vcs::git` passes

---

- [x] 2. Add hg username retrieval implementation

  **What to do**:
  - Implement `get_current_username()` for `HgBackend` in `src/vcs/hg/mod.rs`
  - Use `hg config ui.username` CLI command and parse output
  - Fallback to `"anonymous"` if not configured or on error
  - Write unit tests

  **Must NOT do**:
  - Don't use native mercurial library (doesn't exist, use CLI like existing hg backend)
  - Don't change trait definition (already added in Task 1)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 1
  - **Parallel Group**: Wave 1
  - **Blocks**: None
  - **Blocked By**: Task 1 (trait method must exist)

  **References**:
  - `src/vcs/hg/mod.rs` - Existing hg CLI usage pattern (see `info()` method)
  - `std::process::Command` usage pattern in codebase

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: Returns hg ui.username when configured
  - [ ] Test: Returns `"anonymous"` when not configured

  **GREEN Phase**:
  - [ ] Implement using `hg config ui.username` CLI

  **QA Scenarios**:

  ```
  Scenario: Hg username retrieval success
    Tool: cargo test
    Preconditions: Hg repo with `ui.username = "Bob Coder"` in .hg/hgrc
    Steps:
      1. Create temp dir, init hg repo
      2. Configure ui.username
      3. Call `HgBackend::new(&path)?.get_current_username()`
    Expected Result: Returns `Ok("Bob Coder")`
    Evidence: .sisyphus/evidence/task-2-hg-username.txt

  Scenario: Hg username fallback
    Tool: cargo test
    Preconditions: Hg repo without ui.username
    Steps:
      1. Create temp dir, init hg repo
      2. Call `get_current_username()`
    Expected Result: Returns `Ok("anonymous")`
    Evidence: .sisyphus/evidence/task-2-hg-fallback.txt
  ```

  **Evidence**: .sisyphus/evidence/task-2-hg-*.txt

  **Commit**: YES (Commit 2)
  - Message: `feat(vcs): implement get_current_username() for hg backend`
  - Files: `src/vcs/hg/mod.rs`
  - Pre-commit: `cargo test vcs::hg` passes

---

- [x] 3. Add jj username retrieval implementation

  **What to do**:
  - Implement `get_current_user()` for `JjBackend` in `src/vcs/jj/mod.rs`
  - Use `jj config get user.name` CLI command
  - Fallback to `"anonymous"` if not configured or on error
  - Write unit tests

  **Must NOT do**:
  - Don't use native jj library (doesn't exist, use CLI)
  - Don't change trait definition

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 1
  - **Parallel Group**: Wave 1
  - **Blocks**: None
  - **Blocked By**: Task 1

  **References**:
  - `src/vcs/jj/mod.rs` - Existing jj CLI usage pattern

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: Returns jj user.name when configured
  - [ ] Test: Returns `"anonymous"` when not configured

  **GREEN Phase**:
  - [ ] Implement using `jj config get user.name` CLI

  **QA Scenarios**:

  ```
  Scenario: Jj username retrieval success
    Tool: cargo test
    Preconditions: Jj repo with `user.name` configured
    Steps:
      1. Create temp dir, init jj repo
      2. Run `jj config set user.name "Charlie Dev"`
      3. Call `JjBackend::new(&path)?.get_current_username()`
    Expected Result: Returns `Ok("Charlie Dev")`
    Evidence: .sisyphus/evidence/task-3-jj-username.txt

  Scenario: Jj username fallback
    Tool: cargo test
    Preconditions: Jj repo without user.name
    Steps:
      1. Create temp dir, init jj repo
      2. Call `get_current_username()`
    Expected Result: Returns `Ok("anonymous")`
    Evidence: .sisyphus/evidence/task-3-jj-fallback.txt
  ```

  **Evidence**: .sisyphus/evidence/task-3-jj-*.txt

  **Commit**: YES (Commit 3)
  - Message: `feat(vcs): implement get_current_username() for jj backend`
  - Files: `src/vcs/jj/mod.rs`
  - Pre-commit: `cargo test vcs::jj` passes

---

- [x] 4. Add config options (persistence, in_repo_retention_days)

  **What to do**:
  - Add `persistence: String` field to `AppConfig` struct in `src/config/mod.rs`
  - Add `in_repo_retention_days: u32` field
  - Add both to `KNOWN_KEYS` array (atomically)
  - Add parsing logic: `read_string("persistence")`, `read_u32("in_repo_retention_days")` with defaults
  - Default `persistence` = `"local"`, default `in_repo_retention_days` = `0` (keep forever)
  - Write unit tests for config parsing

  **Must NOT do**:
  - Don't add fields without adding to `KNOWN_KEYS` (users will get "unknown config key" warnings)
  - Don't change existing config parsing logic (add alongside)
  - Don't use `unwrap()` in parsing

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 1
  - **Parallel Group**: Wave 1
  - **Blocks**: Task 6, Task 7
  - **Blocked By**: None

  **References**:
  - `src/config/mod.rs` - Existing `AppConfig` struct and `KNOWN_KEYS`
  - Pattern: `read_string("key")` and `read_bool("key")` in `load_config_from_path()`
  - Create `read_u32()` helper following same pattern as `read_bool()`

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: Config with `persistence = "repo"` parses correctly
    ```rust
    let config = parse_config_toml(r#"persistence = "repo""#)?;
    assert_eq!(config.persistence, "repo");
    ```
  - [ ] Test: Config with `in_repo_retention_days = 30` parses correctly
  - [ ] Test: Default values when not specified
  - [ ] Test: Invalid persistence value logs warning but doesn't fail

  **GREEN Phase**:
  - [ ] Add fields to `AppConfig`
  - [ ] Add to `KNOWN_KEYS`
  - [ ] Add parsing with defaults

  **QA Scenarios**:

  ```
  Scenario: Config persistence option parsing
    Tool: cargo test
    Preconditions: Valid TOML config
    Steps:
      1. Parse config with `persistence = "repo"`
      2. Assert field value is "repo"
    Expected Result: `config.persistence == "repo"`
    Evidence: .sisyphus/evidence/task-4-config-persistence.txt

  Scenario: Config retention option parsing
    Tool: cargo test
    Steps:
      1. Parse config with `in_repo_retention_days = 7`
      2. Assert field value is 7
    Expected Result: `config.in_repo_retention_days == 7`
    Evidence: .sisyphus/evidence/task-4-config-retention.txt

  Scenario: Config defaults
    Tool: cargo test
    Steps:
      1. Parse empty/minimal config
      2. Check default values
    Expected Result: `persistence == "local"`, `in_repo_retention_days == 0`
    Evidence: .sisyphus/evidence/task-4-config-defaults.txt
  ```

  **Evidence**: .sisyphus/evidence/task-4-config-*.txt

  **Commit**: YES (Commit 4)
  - Message: `feat(config): add persistence and in_repo_retention_days options`
  - Files: `src/config/mod.rs`
  - Pre-commit: `cargo test config` passes

---

- [ ] 5. Add --in-repo CLI flag to CliArgs

  **What to do**:
  - Add `in_repo: bool` field to `CliArgs` struct in `src/theme/mod.rs`
  - Add parsing in `parse_cli_args_from()`: match `"--in-repo"` → set `in_repo = true`
  - Add to help output (if applicable)
  - Write tests for CLI parsing

  **Must NOT do**:
  - Don't change existing CLI arg parsing logic
  - Don't add short flag without explicit decision (no `-r`, just `--in-repo`)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 2
  - **Parallel Group**: Wave 2 (with Tasks 5, 6)
  - **Blocks**: Task 7
  - **Blocked By**: None

  **References**:
  - `src/theme/mod.rs` - `CliArgs` struct and `parse_cli_args_from()` function
  - Pattern: see `--theme` parsing for example

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: `--in-repo` flag sets `in_repo = true`
    ```rust
    let args = parse_cli_args_from(&["--in-repo"]).unwrap();
    assert!(args.in_repo);
    ```
  - [ ] Test: No flag defaults to `in_repo = false`

  **GREEN Phase**:
  - [ ] Add field to `CliArgs`
  - [ ] Add parsing branch

  **QA Scenarios**:

  ```
  Scenario: CLI flag parsing
    Tool: cargo test
    Steps:
      1. Parse args `["--in-repo"]`
      2. Assert `args.in_repo == true`
    Expected Result: Flag recognized and set
    Evidence: .sisyphus/evidence/task-5-cli-flag.txt

  Scenario: CLI flag default
    Tool: cargo test
    Steps:
      1. Parse empty args `[]`
      2. Assert `args.in_repo == false`
    Expected Result: Defaults to false
    Evidence: .sisyphus/evidence/task-5-cli-default.txt
  ```

  **Evidence**: .sisyphus/evidence/task-5-cli-*.txt

  **Commit**: YES (Commit 5)
  - Message: `feat(cli): add --in-repo flag for in-repository persistence`
  - Files: `src/theme/mod.rs`
  - Pre-commit: `cargo test theme` passes

---

- [ ] 6. Add in-repo storage path + filename generation

  **What to do**:
  - Add `get_in_repo_reviews_dir(repo_path: &Path) -> PathBuf` in `src/persistence/storage.rs`
  - Returns `repo_path.join(".tuicr").join("reviews")`
  - Add `in_repo_session_filename(session: &ReviewSession, username: &str) -> String`
  - Format: `{username}_{base_short}_{head_short}_{timestamp}_{uuid_fragment}.json`
  - Use short commit hashes (7 chars) to avoid filename length issues
  - Add `sanitize_filename_component()` helper for username (replace non-alphanumeric with `-`)
  - Write unit tests

  **Must NOT do**:
  - Don't use full 40-char commit hashes (too long)
  - Don't change existing `session_filename()` for local storage
  - Don't use 8-char fingerprint in in-repo filename (redundant, we're already in the repo)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 2
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 8, Task 9
  - **Blocked By**: Task 4 (config), Task 1 (username concept)

  **References**:
  - `src/persistence/storage.rs` - `get_reviews_dir()` and `session_filename()` patterns
  - `src/persistence/storage.rs:142` - `session_filename()` implementation
  - `chrono` crate for timestamp formatting (already used)

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: `get_in_repo_reviews_dir()` returns correct path
    ```rust
    let path = get_in_repo_reviews_dir(Path::new("/repo"));
    assert_eq!(path, PathBuf::from("/repo/.tuicr/reviews"));
    ```
  - [ ] Test: Filename format with commit range
    ```rust
    // session with base_commit="abc123...", commit_range=["abc123...", "def456..."]
    let filename = in_repo_session_filename(&session, "alice");
    assert!(filename.starts_with("alice_abc123_def456_"));
    assert!(filename.ends_with(".json"));
    ```
  - [ ] Test: Filename sanitization for special characters
    ```rust
    let sanitized = sanitize_filename_component("user@email.com");
    assert_eq!(sanitized, "user-email-com");
    ```
  - [ ] Test: Empty username falls back to "anonymous"

  **GREEN Phase**:
  - [ ] Implement path resolution function
  - [ ] Implement filename generation with short hashes
  - [ ] Add sanitization helper

  **QA Scenarios**:

  ```
  Scenario: In-repo path resolution
    Tool: cargo test
    Steps:
      1. Call `get_in_repo_reviews_dir(Path::new("/tmp/test-repo"))`
    Expected Result: Returns `PathBuf::from("/tmp/test-repo/.tuicr/reviews")`
    Evidence: .sisyphus/evidence/task-6-path-resolution.txt

  Scenario: Filename generation with commit range
    Tool: cargo test
    Preconditions: Session with base="abc123def..." (40 chars), range=["abc123...", "def456..."]
    Steps:
      1. Generate filename with username "bob"
    Expected Result: Format: `bob_abc123_def456_YYYYMMDD_HHMMSS_xxxx.json` (short hashes)
    Evidence: .sisyphus/evidence/task-6-filename-format.txt

  Scenario: Filename sanitization edge cases
    Tool: cargo test
    Steps:
      1. Test with "José García@corp.com" → "Jos-Garc-a-corp-com"
      2. Test with empty string → "anonymous"
      3. Test with "user_name" → "user_name" (underscores preserved)
    Expected Result: All special chars replaced, empty falls back
    Evidence: .sisyphus/evidence/task-6-sanitization.txt
  ```

  **Evidence**: .sisyphus/evidence/task-6-*.txt

  **Commit**: YES (Commit 6)
  - Message: `feat(persistence): add in-repo path resolution and filename generation`
  - Files: `src/persistence/storage.rs`
  - Pre-commit: `cargo test persistence::storage` passes

---

 - [x] 7. Wire config + CLI through App persistence mode resolution

  **What to do**:
  - Add `persistence_mode: PersistenceMode` enum to `src/app.rs` or new file (Local, Repo)
  - In `main.rs`, resolve final persistence mode:
    - CLI `--in-repo` flag wins if present → `Repo`
    - Otherwise use config `persistence` value
    - Default to `Local` if neither specified
  - Store resolved mode in `App` struct
  - Add tests for precedence logic

  **Must NOT do**:
  - Don't change `App::new()` signature significantly
  - Don't break existing local storage behavior

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO - Sequential after Wave 2 prereqs
  - **Blocks**: Task 8, Task 9
  - **Blocked By**: Task 1 (username concept), Task 4 (config), Task 5 (CLI flag)

  **References**:
  - `src/main.rs` - CLI parsing and App initialization
  - `src/app.rs` - App struct definition
  - `src/config/mod.rs` - AppConfig usage pattern

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: CLI `--in-repo` overrides config `persistence = "local"`
  - [ ] Test: Config `persistence = "repo"` sets mode when no CLI flag
  - [ ] Test: Default is Local when neither specified
  - [ ] Test: Invalid config value logs warning, defaults to Local

  **GREEN Phase**:
  - [ ] Add `PersistenceMode` enum
  - [ ] Add resolution logic in main.rs
  - [ ] Store in App struct

  **QA Scenarios**:

  ```
  Scenario: CLI flag precedence
    Tool: cargo test
    Preconditions: Config has `persistence = "local"`, CLI has `--in-repo`
    Steps:
      1. Resolve persistence mode
    Expected Result: Mode is `Repo` (CLI wins)
    Evidence: .sisyphus/evidence/task-7-cli-precedence.txt

  Scenario: Config-only mode
    Tool: cargo test
    Preconditions: Config has `persistence = "repo"`, no CLI flag
    Steps:
      1. Resolve persistence mode
    Expected Result: Mode is `Repo`
    Evidence: .sisyphus/evidence/task-7-config-mode.txt

  Scenario: Default mode
    Tool: cargo test
    Preconditions: No config, no CLI flag
    Steps:
      1. Resolve persistence mode
    Expected Result: Mode is `Local`
    Evidence: .sisyphus/evidence/task-7-default-mode.txt
  ```

  **Evidence**: .sisyphus/evidence/task-7-*.txt

  **Commit**: YES (Commit 7)
  - Message: `feat(app): resolve persistence mode from CLI and config`
  - Files: `src/main.rs`, `src/app.rs` (+ possibly new enum file)
  - Pre-commit: `cargo test` passes

---

- [x] 8. Implement in-repo save (JSON + MD dual export)

  **What to do**:
  - Add `save_session_to_repo(session: &ReviewSession, vcs: &dyn VcsBackend, diff_source: &DiffSource) -> Result<PathBuf>` in `src/persistence/storage.rs`
  - Get username via `vcs.get_current_username()`
  - Generate in-repo filename using `in_repo_session_filename()`
  - Create `.tuicr/reviews/` directory if doesn't exist (use `fs::create_dir_all`)
  - **Sanitize `repo_path`**: Convert to relative path (e.g., `"."`) before serializing
  - Serialize JSON with `serde_json::to_string_pretty`
  - Generate markdown via `generate_export_content()` (from `output/markdown.rs`)
  - Write both files atomically (write to temp, then rename to avoid partial writes)
  - Return path to saved JSON file
  - Write unit tests

  **Must NOT do**:
  - Don't change `save_session()` signature (maintain backward compatibility)
  - Don't use `unwrap()` - propagate errors
  - Don't auto-commit files to git
  - Don't auto-add to `.gitignore`

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 3 (with Task 9)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 11
  - **Blocked By**: Task 1 (username), Task 6 (filename), Task 7 (mode resolution)

  **References**:
  - `src/persistence/storage.rs:174` - `save_session()` for local storage pattern
  - `src/output/markdown.rs:16` - `generate_export_content()` signature
  - `src/model/review.rs` - ReviewSession fields
  - `std::fs::create_dir_all`, `std::fs::rename` for atomic writes

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: Saves both `.json` and `.md` files
    ```rust
    let json_path = save_session_to_repo(&session, &vcs, &diff_source)?;
    let md_path = json_path.with_extension("md");
    assert!(json_path.exists());
    assert!(md_path.exists());
    ```
  - [ ] Test: JSON has relative repo_path (not absolute)
    ```rust
    let content = fs::read_to_string(&json_path)?;
    let saved: ReviewSession = serde_json::from_str(&content)?;
    assert_eq!(saved.repo_path, PathBuf::from("."));
    ```
  - [ ] Test: Markdown contains expected review content
  - [ ] Test: Creates `.tuicr/reviews/` if missing
  - [ ] Test: Uses username in filename

  **GREEN Phase**:
  - [ ] Implement `save_session_to_repo()` function
  - [ ] Add repo_path sanitization logic
  - [ ] Wire markdown export

  **QA Scenarios**:

  ```
  Scenario: Dual-file save
    Tool: cargo test
    Preconditions: Fresh git repo with persistence="repo"
    Steps:
      1. Create ReviewSession with comments
      2. Call `save_session_to_repo(&session, &vcs, &diff_source)`
    Expected Result:
      - `.tuicr/reviews/` directory created
      - Both `.json` and `.md` files exist with matching base names
      - JSON contains relative repo_path (".")
    Evidence: .sisyphus/evidence/task-8-dual-save.txt

  Scenario: Repo path sanitization
    Tool: cargo test
    Steps:
      1. Create session with absolute repo_path
      2. Save to repo
      3. Load saved JSON
    Expected Result: `repo_path` field is relative ("." or "repo-root-relative")
    Evidence: .sisyphus/evidence/task-8-sanitization.txt

  Scenario: Username in filename
    Tool: cargo test
    Preconditions: Git user.name = "Alice"
    Steps:
      1. Save session
      2. List `.tuicr/reviews/`
    Expected Result: Filename starts with "Alice_"
    Evidence: .sisyphus/evidence/task-8-username-filename.txt
  ```

  **Evidence**: .sisyphus/evidence/task-8-*.txt

  **Commit**: YES (Commit 8)
  - Message: `feat(persistence): implement in-repo save with JSON + MD export`
  - Files: `src/persistence/storage.rs`, possibly `src/model/review.rs` (if sanitizer needed)
  - Pre-commit: `cargo test persistence::storage` passes

---

- [ ] 9. Implement in-repo load with user-scoping + retention

  **What to do**:
  - Add `load_latest_in_repo_session(vcs: &dyn VcsBackend, repo_path: &Path, diff_source: &DiffSource, commit_range: Option<&[String]>, retention_days: u32) -> Result<Option<(PathBuf, ReviewSession)>>` in `src/persistence/storage.rs`
  - Scan `.tuicr/reviews/` for `.json` files
  - Parse filenames to extract username
  - Filter: only files where `username == current_user` (match via `vcs.get_current_username()`)
  - Parse JSON and validate: diff_source matches, commit_range matches (if provided)
  - Apply retention: skip (don't load) files older than `retention_days` (0 = no retention, load all)
  - **CRITICAL**: Do NOT delete files (unlike local storage) - just filter them out
  - Return most recent matching session (by `updated_at` field, not filesystem mtime)
  - Write unit tests

  **Must NOT do**:
  - Don't use filesystem mtime for retention (git clone resets times)
  - Don't delete old files from `.tuicr/reviews/`
  - Don't load other users' sessions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 3 (with Task 8)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 11
  - **Blocked By**: Task 1 (username), Task 6 (filename), Task 7 (mode)

  **References**:
  - `src/persistence/storage.rs:192` - `load_latest_session_for_context()` pattern
  - `src/persistence/storage.rs:228-236` - Local retention (deletes files) - DON'T copy this for in-repo
  - Use session `created_at`/`updated_at` fields for age calculation

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: Loads current user's session only
    ```rust
    // Files: alice_xxx.json, bob_xxx.json
    let result = load_latest_in_repo_session(&vcs, &path, &diff_source, None, 0)?;
    assert!(result.is_some());
    // Should load alice's session, not bob's
    ```
  - [ ] Test: Applies retention (skips old files)
    ```rust
    // File from 30 days ago, retention_days = 7
    let result = load_latest_in_repo_session(..., 7)?;
    assert!(result.is_none()); // Skipped, not deleted
    // File still exists on disk!
    ```
  - [ ] Test: Returns None when no matching session
  - [ ] Test: Matches by diff_source and commit_range
  - [ ] Test: Files not deleted after retention filtering

  **GREEN Phase**:
  - [ ] Implement `load_latest_in_repo_session()`
  - [ ] Add user-scoped filtering
  - [ ] Add retention filtering (no delete)

  **QA Scenarios**:

  ```
  Scenario: User-scoped loading
    Tool: cargo test
    Preconditions: `.tuicr/reviews/` has alice_*.json and bob_*.json
    Steps:
      1. Set git user.name = "alice"
      2. Call load function
    Expected Result: Returns alice's session, ignores bob's
    Evidence: .sisyphus/evidence/task-9-user-scoped.txt

  Scenario: Retention filtering without deletion
    Tool: cargo test
    Preconditions: Old session from 30 days ago, retention = 7 days
    Steps:
      1. Attempt to load with retention_days = 7
      2. Check file still exists on disk after load
    Expected Result: Returns None (skipped), file NOT deleted
    Evidence: .sisyphus/evidence/task-9-retention.txt

  Scenario: Zero retention (keep forever)
    Tool: cargo test
    Preconditions: Old session from 1 year ago, retention = 0
    Steps:
      1. Call load with retention_days = 0
    Expected Result: Loads the old session
    Evidence: .sisyphus/evidence/task-9-no-retention.txt
  ```

  **Evidence**: .sisyphus/evidence/task-9-*.txt

  **Commit**: YES (Commit 9)
  - Message: `feat(persistence): implement in-repo load with user-scoping and retention`
  - Files: `src/persistence/storage.rs`
  - Pre-commit: `cargo test persistence::storage` passes

---

- [ ] 10. Auto-filter .tuicr/ from diff view in tuicrignore.rs

  **What to do**:
  - Modify `apply_tuicrignore()` in `src/tuicrignore.rs` to always exclude `.tuicr/` paths
  - Add early return or check: if path starts with `.tuicr/`, return `true` (excluded)
  - This happens BEFORE user `.tuicrignore` processing
  - Write unit tests

  **Must NOT do**:
  - Don't rely on user's `.tuicrignore` to exclude `.tuicr/` (they might forget)
  - Don't change the filtering API signature

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES - Wave 4 (can run alongside Task 11, or even earlier)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 11
  - **Blocked By**: None (independent)

  **References**:
  - `src/tuicrignore.rs` - `apply_tuicrignore()` function
  - `src/tuicrignore.rs` - `load_tuicrignore()` and filtering logic

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: `.tuicr/reviews/file.json` is filtered out
    ```rust
    let filter = load_tuicrignore(&repo_root)?;
    assert!(apply_tuicrignore(&filter, Path::new(".tuicr/reviews/foo.json")));
    ```
  - [ ] Test: `.tuicr/` at any depth is filtered
    ```rust
    assert!(apply_tuicrignore(&filter, Path::new("src/.tuicr/nested.txt")));
    ```
  - [ ] Test: Other files are not affected
    ```rust
    assert!(!apply_tuicrignore(&filter, Path::new("src/main.rs")));
    ```

  **GREEN Phase**:
  - [ ] Add hardcoded `.tuicr/` exclusion in `apply_tuicrignore()`

  **QA Scenarios**:

  ```
  Scenario: .tuicr/ auto-exclusion
    Tool: cargo test
    Steps:
      1. Create filter for repo
      2. Test `apply_tuicrignore(Path::new(".tuicr/reviews/session.json"))`
    Expected Result: Returns `true` (excluded)
    Evidence: .sisyphus/evidence/task-10-tuicrignore.txt

  Scenario: Nested .tuicr exclusion
    Tool: cargo test
    Steps:
      1. Test path containing .tuicr at any level
    Expected Result: All `.tuicr/` paths excluded
    Evidence: .sisyphus/evidence/task-10-nested.txt
  ```

  **Evidence**: .sisyphus/evidence/task-10-*.txt

  **Commit**: YES (Commit 10)
  - Message: `feat(ignore): auto-filter .tuicr/ directory from diff view`
  - Files: `src/tuicrignore.rs`
  - Pre-commit: `cargo test tuicrignore` passes

---

- [ ] 11. Wire save/load dispatch through handler.rs and main.rs

  **What to do**:
  - In `src/handler.rs`: Update save handlers (`:w`, `:wq`, `:x`, `ZZ`) to check `app.persistence_mode`
    - If `Local`: call `save_session(&app.session)` (existing behavior)
    - If `Repo`: call `save_session_to_repo(&app.session, &*app.vcs, &app.diff_source)` (new)
  - In `src/main.rs`: Update session loading on startup
    - If `Local`: call `load_latest_session_for_context(...)` (existing)
    - If `Repo`: call `load_latest_in_repo_session(...)` (new)
  - Add error handling for in-repo failures (permissions, etc.)
  - Write integration tests

  **Must NOT do**:
  - Don't change existing local storage behavior
  - Don't break CLI without persistence flag (should still use local)
  - Don't auto-fallback on in-repo failure (fail fast with clear error)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO - Sequential after Wave 3
  - **Blocks**: Final Verification
  - **Blocked By**: Task 7 (mode), Task 8 (save), Task 9 (load), Task 10 (filter)

  **References**:
  - `src/handler.rs:157,164` - Existing save calls
  - `src/main.rs:266` - Existing save call on `ZZ`
  - `src/app.rs:App::new()` - Session loading on startup
  - `src/persistence/mod.rs` - Function re-exports

  **Acceptance Criteria**:

  **RED Phase**:
  - [ ] Test: Local mode uses local storage (existing tests should still pass)
  - [ ] Test: Repo mode uses `.tuicr/reviews/`
    ```rust
    // Integration test: create temp repo, set mode=Repo, save, verify files created
    ```
  - [ ] Test: Mode resolution on startup loads correct session

  **GREEN Phase**:
  - [ ] Add dispatch logic in handler.rs
  - [ ] Add dispatch logic in main.rs startup

  **QA Scenarios**:

  ```
  Scenario: Full in-repo workflow
    Tool: cargo test (integration)
    Preconditions: Temp git repo with user.name configured, persistence="repo"
    Steps:
      1. Initialize App with in-repo mode
      2. Add a comment to a file
      3. Trigger save (`:w` equivalent)
      4. Verify `.tuicr/reviews/` has both .json and .md
      5. Restart App (simulates reload)
      6. Verify session loads with comment intact
    Expected Result: Complete round-trip save and load
    Evidence: .sisyphus/evidence/task-11-full-workflow.txt

  Scenario: Local mode still works
    Tool: cargo test
    Preconditions: persistence="local" (default)
    Steps:
      1. Save session
      2. Verify files in ~/.local/share/tuicr/reviews/
    Expected Result: Existing behavior preserved
    Evidence: .sisyphus/evidence/task-11-local-mode.txt
  ```

  **Evidence**: .sisyphus/evidence/task-11-*.txt

  **Commit**: YES (Commit 11)
  - Message: `feat(integration): wire in-repo persistence through handlers and main`
  - Files: `src/handler.rs`, `src/main.rs`, `src/persistence/mod.rs`
  - Pre-commit: `cargo test` passes (all tests)

---

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  **What to verify**:
  - Must Have [8/8]:
    - [ ] Config options exist (persistence, in_repo_retention_days)
    - [ ] VcsBackend has get_current_username() with all 3 impls
    - [ ] CLI has --in-repo flag
    - [ ] In-repo path resolution exists
    - [ ] In-repo save creates both JSON + MD
    - [ ] In-repo load with user-scoping works
    - [ ] .tuicr/ is auto-filtered from diff
    - [ ] Integration dispatch works
  - Must NOT Have [7/7]:
    - [ ] No migration code for existing sessions
    - [ ] No git add/commit automation
    - [ ] No multi-user session merging
    - [ ] No .tuicr/ initialization prompts
    - [ ] No backward-incompatible ReviewSession changes
    - [ ] No auto-deletion of in-repo files
    - [ ] save_session() signature unchanged
  - Evidence files [11+]: All task evidence exists in .sisyphus/evidence/

  **Tool**: Manual code review + grep
  **Output**: `Must Have [8/8] | Must NOT Have [7/7] | Evidence [N/N] | VERDICT: APPROVE/REJECT`
  **Evidence**: .sisyphus/evidence/f1-compliance.txt

---

- [ ] F2. **Code Quality Review** — `unspecified-high`
  **What to verify**:
  - Build: `cargo build --release` [PASS/FAIL]
  - Tests: `cargo test` [N pass/N fail] (expect 0 failures)
  - Linter: `cargo clippy -- -D warnings` [PASS/FAIL]
  - Code review:
    - [ ] No `unwrap()` in production code (only in tests)
    - [ ] No empty `match` arms or `if let` without else
    - [ ] No unnecessary `.clone()` calls
    - [ ] No unused imports
    - [ ] Error handling uses `?` consistently
    - [ ] No "AI slop" patterns (excessive comments, over-abstraction)

  **Tool**: cargo test, cargo clippy, manual file review
  **Output**: `Build [PASS] | Clippy [PASS] | Tests [N/0] | Quality [CLEAN/N issues] | VERDICT`
  **Evidence**: .sisyphus/evidence/f2-quality.txt (test output, clippy output)

---

- [ ] F3. **Real Integration QA** — `unspecified-high`
  **What to verify** (run in temp git repo):

  ```bash
  # Setup
  mkdir /tmp/tuicr-test && cd /tmp/tuicr-test
  git init && git config user.name "Test User"
  echo "fn main() {}" > src.rs && git add . && git commit -m "init"
  echo "// modified" >> src.rs
  
  # Test 1: In-repo save creates both files
  tuicr --in-repo  # or with config
  # Add comment, press :w
  ls .tuicr/reviews/
  # Expected: 2 files (.json and .md)
  ```

  - [ ] Test 1: Both files exist after save
  - [ ] Test 2: JSON has relative repo_path (not absolute)
  - [ ] Test 3: MD has human-readable content
  - [ ] Test 4: Restart tuicr, session loads correctly
  - [ ] Test 5: .tuicr/ doesn't appear in diff view
  - [ ] Test 6: Local mode still works (default config)
  - [ ] Test 7: CLI --in-repo overrides config
  - [ ] Test 8: Retention doesn't delete files

  **Tool**: Bash script with temp repo setup
  **Output**: `Scenarios [8/8 pass] | VERDICT: PASS/FAIL`
  **Evidence**: .sisyphus/evidence/f3-integration.sh (test script + output)

---

- [ ] F4. **Scope Fidelity Check** — `deep`
  **What to verify**:

  For each task T1-T11:
  - [ ] Read "What to do" in plan
  - [ ] Run `git diff --name-only` to see changed files
  - [ ] Compare: Everything in spec was built? Nothing extra?
  - [ ] Check "Must NOT do" compliance
  
  Cross-task contamination check:
  - [ ] Task N only touches files it should
  - [ ] No Task N code in Task M's files
  - [ ] No accidental changes to unrelated modules

  Guardrails verification:
  - [ ] save_session() signature unchanged (verify with `git diff`)
  - [ ] No migration code added (grep for "migration", "import", "convert")
  - [ ] No git automation (grep for "git add", "git commit", "Command::new")
  - [ ] No .tuicr/ prompts (grep for "stdin", "read_line", interactive)

  **Tool**: git diff, grep, manual review
  **Output**: `Tasks [11/11 compliant] | Contamination [CLEAN/N] | Guardrails [PASS/FAIL] | VERDICT`
  **Evidence**: .sisyphus/evidence/f4-fidelity.txt (per-task verification table)

---

> **STOP**: After F1-F4 complete, present results to user:
> 
> ```
> ## Verification Results
> 
> | Check | Result |
> |-------|--------|
> | Plan Compliance | APPROVE/REJECT |
> | Code Quality | PASS/FAIL |
> | Integration QA | 8/8 scenarios |
> | Scope Fidelity | CLEAN/N issues |
> 
> **Recommendation**: [APPROVE / NEEDS_FIX]
> ```
> 
> Get explicit user "okay" before completing work.

---

## Commit Strategy

```
Commit 1: Add get_current_username() to VcsBackend trait + git impl
  - vcs/traits.rs, vcs/git/mod.rs + tests

Commit 2: Add hg username retrieval implementation  
  - vcs/hg/mod.rs + tests

Commit 3: Add jj username retrieval implementation
  - vcs/jj/mod.rs + tests

Commit 4: Add config options (persistence, in_repo_retention_days)
  - config/mod.rs + tests

Commit 5: Add --in-repo CLI flag
  - theme/mod.rs + tests

Commit 6: In-repo storage path + filename generation
  - persistence/storage.rs + tests

Commit 7: Wire config + CLI through App
  - main.rs, app.rs + tests

Commit 8: In-repo save (JSON + MD dual export)
  - persistence/storage.rs + tests

Commit 9: In-repo load with user-scoping + retention
  - persistence/storage.rs + tests

Commit 10: Auto-filter .tuicr/ from diff view
  - tuicrignore.rs + tests

Commit 11: Wire save/load dispatch
  - handler.rs, main.rs + integration tests
```

---

## Success Criteria

### Verification Commands

```bash
# All tests pass
cargo test

# No lint errors  
cargo clippy -- -D warnings

# Check specific deliverables exist
grep -r "get_current_username" src/vcs/
grep -r "persistence" src/config/mod.rs
grep -r "in_repo" src/
grep -r "tuicr/reviews" src/
```

### Final Checklist
- [ ] All "Must Have" present in codebase
- [ ] All "Must NOT Have" absent from codebase
- [ ] `cargo test` passes (0 failures)
- [ ] `cargo clippy` passes (0 warnings)
- [ ] Evidence files exist for all QA scenarios
- [ ] Documentation updated (README, help_popup.rs, AGENTS.md)
