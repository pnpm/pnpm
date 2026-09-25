---
"pacquet": patch
---

`pnpm import` now warns when package.json lists projects in a "workspaces" array and there is no "pnpm-workspace.yaml". Without that file, the import writes a lockfile for the root project only [#5240](https://github.com/pnpm/pnpm/issues/5240).
