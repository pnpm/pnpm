---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` on CI now fails on an outdated lockfile when `preferFrozenLockfile` is explicitly set to `true`. Setting it to `true` used to let CI update the lockfile [#9072](https://github.com/pnpm/pnpm/pull/9072).
