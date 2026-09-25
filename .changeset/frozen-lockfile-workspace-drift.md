---
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now fails when a workspace package's version no longer satisfies the range that a dependent workspace project declares for it. This includes injected workspace dependencies [#7823](https://github.com/pnpm/pnpm/issues/7823).
