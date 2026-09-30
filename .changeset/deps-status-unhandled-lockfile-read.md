---
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm run` with `verifyDepsBeforeRun` no longer crashes with an unhandled rejection when a lockfile it did not need to compare fails to load.
