---
"pacquet": patch
---

`pnpm install` without `--frozen-lockfile` now links with the same number of worker threads as a frozen install when the project has a `pnpm-workspace.yaml`. Such installs used one thread per core. On a 10-core M1 Max, a warm install of a Nuxt app with a `pnpm-workspace.yaml` got 13% faster.
