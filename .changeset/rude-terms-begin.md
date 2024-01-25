---
"@pnpm/calc-dep-state": major
"pnpm": patch
---

Reduce the length of the side-effects cache key. Instead of saving a stringified object composed from the dependency versions of the package, use the hash calculated from the said object [#7563](https://github.com/pnpm/pnpm/pull/7563).
