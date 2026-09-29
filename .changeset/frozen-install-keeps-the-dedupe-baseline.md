---
"pacquet": patch
---

`pnpm run` and `pnpm exec` no longer install dependencies before every script on CI when `autoDedupe` is enabled. `pnpm install --frozen-lockfile` now keeps the deduplication record left by an earlier install [#16374](https://github.com/pnpm/pnpm/issues/16374).
