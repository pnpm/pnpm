---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.verification": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now writes self-contained workspace and lockfile settings, prunes unused `patchedDependencies`, and preserves workspace-level dependency resolutions on deployed directory dependencies during post-deploy `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` checks.
