---
"pacquet": patch
---

`pnpm unlink` now reinstalls through the selection-aware install pipeline, matching pnpm: it honors `-r` / `--filter`, installs recursively by default inside a workspace, and supports both a shared workspace lockfile and one lockfile per project (`sharedWorkspaceLockfile: false`). Previously it always reinstalled only the active project.
