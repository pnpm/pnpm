---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/lockfile.verification": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now accepts the lockfile it wrote with `excludeLinksFromLockfile` and `linkWorkspacePackages` on, when a project depends on a workspace package through a plain version range. It used to fail with `ERR_PNPM_OUTDATED_LOCKFILE`. The frozen install now links the workspace package again.
