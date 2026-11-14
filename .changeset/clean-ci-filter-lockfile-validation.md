---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm ci` now validates that `pnpm-lock.yaml` exists and matches `package.json` before removing `node_modules`. `pnpm clean` and `pnpm ci` now honor `--filter`, `--filter-prod`, and `--workspace-root`, and clean root `node_modules` in workspaces without a root `package.json`. `pnpm clean --lockfile` now removes per-project lockfiles and lockfiles inside a custom `lockfileDir`.
