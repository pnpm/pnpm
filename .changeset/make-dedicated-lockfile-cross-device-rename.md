---
"@pnpm/lockfile.make-dedicated-lockfile": patch
"pnpm": patch
---

`make-dedicated-lockfile` no longer fails on cross-device or container mount boundaries when backing up and restoring `node_modules`.
