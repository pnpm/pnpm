---
"pacquet": patch
---

`pnpm outdated -r` now wraps the `Dependents` column at 30 columns. A dependency used by many workspace projects listed all of them on one line, which made the table hundreds of columns wide [#14591](https://github.com/pnpm/pnpm/issues/14591).
