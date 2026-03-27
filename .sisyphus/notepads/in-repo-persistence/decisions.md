# 2026-03-27
- Implemented `GitBackend::get_current_username()` by reading the local Git config only, with `anonymous` as a fallback when `user.name` is missing or blank.
- Kept the trait API unchanged because `get_current_username()` was already present on the shared backend trait.
- Decision: Always exclude .tuicr/ paths from diff filtering. Implemented in filter_diff_files by short-circuiting paths that contain a .tuicr component and by ignoring any such paths even if .tuicrignore is absent.
