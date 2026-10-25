---
"@pnpm/deps.status": patch
"pacquet": patch
"pnpm": patch
---

`pnpm run` and `pnpm exec` no longer report the dependencies as out of date after a lockfile change when `dedupePeers` is `true` [#17322](https://github.com/pnpm/pnpm/issues/17322).
