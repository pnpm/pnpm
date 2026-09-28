---
"pacquet": patch
---

`pnpm install` without `--frozen-lockfile` now links with the same number of worker threads as a frozen install. It used one thread per core. On a 10-core M1 Max, a warm install of a Nuxt app got 13% faster.
