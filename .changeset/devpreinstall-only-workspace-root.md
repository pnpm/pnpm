---
"pacquet": patch
---

`pnpm install` no longer runs the `pnpm:devPreinstall` scripts of workspace members when `sharedWorkspaceLockfile` is `false`. Only the workspace root's script runs [pnpm/pnpm#4503](https://github.com/pnpm/pnpm/issues/4503).
