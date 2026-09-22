---
"@pnpm/lockfile.make-dedicated-lockfile": patch
"pnpm": patch
---

`make-dedicated-lockfile` now stages `node_modules` directly without intermediate nested renames that could fail across mount boundaries.
