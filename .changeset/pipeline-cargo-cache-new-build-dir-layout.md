---
"pacquet": patch
---

`pnpm pipeline` now rebuilds local packages and reruns build scripts after it restores a shared Cargo build cache with Cargo 1.100 or newer. Before, a worktree could reuse build-script output from the worktree that created the cache.
