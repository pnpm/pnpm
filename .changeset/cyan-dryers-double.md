---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --fix-lockfile` no longer removes the `deprecated` and `hasBin` fields from lockfile entries [#6600](https://github.com/pnpm/pnpm/issues/6600).
