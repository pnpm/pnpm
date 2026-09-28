---
"pacquet": patch
---

`pnpm install` without `--frozen-lockfile` now links with as many worker threads as a frozen install when the project has a `pnpm-workspace.yaml`. Such installs used one thread per core.
