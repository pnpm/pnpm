---
"@pnpm/installing.commands": patch
"@pnpm/installing.context": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.modules-cleaner": patch
"pnpm": patch
"pacquet": patch
---

`pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also honors `--filter`, `-r`, and `sharedWorkspaceLockfile: false` in workspaces.
