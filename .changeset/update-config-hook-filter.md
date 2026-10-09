---
"pacquet": patch
---

`pnpm install` and other recursive commands now use the `filter` and `filterProd` selectors that an `updateConfig` hook sets. A `--filter` or `--filter-prod` given on the command line still takes precedence over the hook [#16792](https://github.com/pnpm/pnpm/issues/16792).
