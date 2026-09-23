---
"pacquet": patch
---

`pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now work in a workspace project that keeps its own lockfile (`sharedWorkspaceLockfile: false`). `pnpm patch` failed there with `ERR_PNPM_PATCH_NO_LOCKFILE` after a successful install, and the reinstall after committing or removing a patch went to the workspace root's `node_modules` [#9926](https://github.com/pnpm/pnpm/issues/9926).
