---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --fix-lockfile` preserves `hasBin` and `deprecated` package fields [#6600](https://github.com/pnpm/pnpm/issues/6600).
