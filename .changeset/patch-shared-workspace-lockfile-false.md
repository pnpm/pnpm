---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/patching.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm patch`, `pnpm patch-commit`, `pnpm patch-remove`, and `pnpm install` in workspaces with `sharedWorkspaceLockfile: false`. Patch files and `patchedDependencies` entries now resolve against the workspace root, and unused-patch checks now evaluate configured patches across all workspace projects.
