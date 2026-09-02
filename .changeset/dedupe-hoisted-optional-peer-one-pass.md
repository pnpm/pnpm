---
"pacquet": patch
---

`pnpm dedupe` now converges in one pass when it re-resolves a lockfile created by pnpm 11, so a second run no longer changes the lockfile [#14455](https://github.com/pnpm/pnpm/issues/14455).
