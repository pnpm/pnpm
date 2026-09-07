---
"pacquet": minor
---

`pnpm pipeline` can reuse local Cargo build state between worktrees with the `tasks.<name>.cargoTargetDir` setting. Restored build directories remain usable after the cache is deleted. Root tasks can participate with `includeWorkspaceRoot: true`.
