---
"@pnpm/deps.status": patch
"@pnpm/workspace.state": patch
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

A repeat `pnpm install` no longer reports "Already up to date" after `virtualStoreDirMaxLength` changes, `dangerouslyAllowAllBuilds` is turned on, or `trustLockfile` is turned off. It renames the virtual store directories, runs the builds it ignored before, or verifies the lockfile.
