---
"pacquet": patch
---

`pnpm dedupe --check` now passes right after `pnpm dedupe` when deduplication merges variants of a package that differ only in their peers. A lockfile key whose peer suffix named a merged variant now names the variant that replaced it [#16356](https://github.com/pnpm/pnpm/issues/16356).
