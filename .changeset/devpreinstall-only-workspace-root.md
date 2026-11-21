---
"pacquet": patch
---

When `sharedWorkspaceLockfile` is `false`, `pnpm install` now runs the workspace root's `pnpm:devPreinstall` script once, before any project is installed, including when a filter excludes the root. Workspace members no longer run theirs [pnpm/pnpm#4503](https://github.com/pnpm/pnpm/issues/4503).
