---
"@pnpm/core": patch
---

Don't validate (and possibly purge) modules directory in operations that do not mutate the structure (e.g. `mutateModules({ ... }, { ..., lockfileOnly: true })`) [#8657](https://github.com/pnpm/pnpm/pull/8657).
