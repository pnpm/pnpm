---
"pacquet": patch
---

`pnpm dedupe` converges in one pass when it re-resolves a lockfile written before optional peers were hoisted: a direct dependency whose locked peer suffix did not record a hoisted optional peer now gains it in the same run as its transitive dependencies, so a second `pnpm dedupe` no longer changes the lockfile [#14455](https://github.com/pnpm/pnpm/issues/14455).
