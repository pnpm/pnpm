---
"pnpm": patch
---

Optimize peers resolution to avoid out-of-memory exceptions in some rare cases, when there are too many circular dependencies and peer dependencies [#7149](https://github.com/pnpm/pnpm/pull/7149).
