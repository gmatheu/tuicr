# 2026-03-27
- Git username tests need isolation from global config state; using `Repository::init()` plus temp directories keeps the coverage deterministic.
- No blockers encountered while implementing automatic filtering of .tuicr/ in ignore pass.
- 20260327 Task 11: Discovered save/load incompatibility — `save_session_in_repo` used local filename format while `load_latest_in_repo_session` expected user-prefixed format. Fixed as part of the dispatch wiring.
