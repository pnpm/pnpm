---
"pacquet": patch
---

`pnpm dedupe` accepts the `pnpm install` options that pnpm documents for it — `--lockfile-only`, `--ignore-scripts`, `--offline`, and `--prefer-offline` — instead of rejecting them with `unexpected argument`. Without `--lockfile-only`, `pnpm dedupe` now also updates `node_modules`, as an install does [#14107](https://github.com/pnpm/pnpm/issues/14107).
