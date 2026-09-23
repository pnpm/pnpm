---
"@pnpm/lockfile.make-dedicated-lockfile": patch
"pnpm": patch
---

`make-dedicated-lockfile` now restores `package.json` when it cannot move the original `node_modules` back. If the install also failed, the error names the directory that still holds the original `node_modules`.
