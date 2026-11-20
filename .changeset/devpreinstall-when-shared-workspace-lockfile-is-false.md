---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm install` now runs the workspace root's `pnpm:devPreinstall` script when `sharedWorkspaceLockfile` is `false` [pnpm/pnpm#4503](https://github.com/pnpm/pnpm/issues/4503).
