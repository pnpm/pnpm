---
"@pnpm/lockfile.make-dedicated-lockfile": patch
"pnpm": patch
---

`make-dedicated-lockfile` now restores `package.json` when it cannot move the original `node_modules` back to its place. The error then names `.tmp_node_modules`, where the original `node_modules` was left. The command refuses to run while that directory exists, so a retry cannot overwrite it.
