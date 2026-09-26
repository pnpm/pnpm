---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm:devPreinstall` now runs in the workspace root when `shared-workspace-lockfile` is false [pnpm/pnpm#4503](https://github.com/pnpm/pnpm/issues/4503).
