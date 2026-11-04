---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/lockfile.verification": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now accepts a lockfile written with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a `catalog:` entry. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE` once `node_modules` was removed.
