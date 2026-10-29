---
"@pnpm/workspace.state": patch
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

A repeat `pnpm install` no longer reports "Already up to date" after a local tarball or directory that only a pnpmfile `readPackage` hook depends on changes [pnpm/tasks#227](https://github.com/pnpm/tasks/issues/227).
