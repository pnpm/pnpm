---
"@pnpm/deps.graph-hasher": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Restore cached build artifacts when reinstalling a workspace that uses separate lockfiles [#12942](https://github.com/pnpm/pnpm/issues/12942).
