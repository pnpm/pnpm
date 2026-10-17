---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of an injected workspace package no longer match the lockfile. Before, it installed the stale dependencies when `sharedWorkspaceLockfile` was `false` [#17008](https://github.com/pnpm/pnpm/issues/17008).
