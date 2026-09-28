---
"pacquet": patch
---

`pnpm install` without `--frozen-lockfile` now links with as many worker threads as a frozen install when the project has a `pnpm-workspace.yaml`. Such installs used one thread per core. On a 10-core M1 Max, a fresh install of a project with 1,352 packages got 16% faster.
