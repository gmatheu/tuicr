Summary of last feature improvements

- Request: Auto-filter .tuicr/ from the ignore pass in tuicrignore by default to prevent leakage of repo artifacts into diffs.
- What I changed: Updated filter_diff_files in src/tuicrignore.rs to always exclude any path containing the .tuicr directory, even when .tuicrignore is absent. When .tuicrignore exists, respect its rules in addition to the hard exclusion.
- Tests: Added unit tests to cover root-level and nested .tuicr/ paths, plus a test with an ignore file present. All tests pass locally (cargo test -> 312 passed).
- Verification: Ran full test suite; the ignore behavior now excludes .tuicr paths by default and preserves existing ignore semantics for other patterns.
- Notes: Changes are isolated to ignore filtering logic; no other ignore behavior was altered.
