---
"@pnpm/deps.status": patch
"@pnpm/workspace.state": patch
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

A repeat `pnpm install` now renames the virtual store directories after `virtualStoreDirMaxLength` changes. It now runs the builds it skipped before after `dangerouslyAllowAllBuilds` is turned on. It now verifies the lockfile after `trustLockfile` is turned off. Before, it reported "Already up to date" in each of these cases.
