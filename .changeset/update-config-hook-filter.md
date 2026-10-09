---
"pacquet": patch
---

Fixed a regression where `pnpm install` and other recursive commands ignored the `filter` and `filterProd` that an `updateConfig` hook sets [#16792](https://github.com/pnpm/pnpm/issues/16792).
