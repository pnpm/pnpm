---
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

Fixed `optimisticRepeatInstall` skipping `pnpm-lock.yaml` merge conflict resolution when the existing `node_modules` state appears up to date.
