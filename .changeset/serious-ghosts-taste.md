---
"@pnpm/resolve-dependencies": patch
"@pnpm/core": patch
"@pnpm/lockfile-utils": patch
"@pnpm/pick-fetcher": patch
"pnpm": patch
---

Don't update git-hosted dependencies when adding an unrelated dependency [#7008](https://github.com/pnpm/pnpm/issues/7008).
