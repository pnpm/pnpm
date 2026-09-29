---
"pacquet": patch
---

`pnpm run` and `pnpm exec` no longer install dependencies before every script after `pnpm install --frozen-lockfile` when `autoDedupe` is enabled. The pre-run dependency check was asking for a deduplication baseline a frozen install cannot record. A frozen install that has nothing to materialize also keeps the baseline a previous install recorded instead of dropping it [#16374](https://github.com/pnpm/pnpm/issues/16374).
