---
"pnpm": patch
---

`pnpm install` drops an optional peer when you remove its last provider. `pnpm dedupe` and `pnpm install --fix-lockfile` also remove stale optional peer bindings from existing lockfiles [#18482](https://github.com/pnpm/pnpm/issues/18482).
