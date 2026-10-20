---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/patching.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed patching in workspaces with `sharedWorkspaceLockfile: false`. `pnpm patch`, `pnpm patch-commit`, and `pnpm patch-remove` now resolve patch files and `patchedDependencies` entries at the workspace root. `pnpm install` now checks configured patches across all workspace projects before reporting unused patches.
