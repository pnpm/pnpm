---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.verification": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` failing inside a directory created by `pnpm deploy`. `pnpm deploy` now writes self-contained workspace and lockfile settings. `pnpm deploy` also prunes unused `patchedDependencies` and preserves workspace-level dependency resolutions on deployed directory dependencies.
