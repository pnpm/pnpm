---
"@pnpm/pkg-manifest.utils": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/deps.inspection.outdated": patch
"@pnpm/types": patch
"pnpm": patch
"pacquet": patch
---

Added a `--peer` flag to `pnpm update` to update ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).
