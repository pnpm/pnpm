---
"@pnpm/resolve-dependencies": patch
"@pnpm/core": patch
"@pnpm/lockfile-utils": patch
"@pnpm/pick-fetcher": patch
"pnpm": patch
---

Don't update git protocol dependencies when adding unrelated dependency [#7008](https://github.com/pnpm/pnpm/issues/7008).
