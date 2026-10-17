---
"@pnpm/lockfile.verification": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when a dependency of an injected workspace package changes and `sharedWorkspaceLockfile` is `false`. Previously, the install succeeded and kept the old resolution [#17008](https://github.com/pnpm/pnpm/issues/17008).
