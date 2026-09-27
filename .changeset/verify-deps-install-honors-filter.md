---
"@pnpm/deps.status": patch
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

When `verifyDepsBeforeRun` triggers an install before a filtered `pnpm run` or `pnpm exec`, pnpm now installs only the selected projects and their dependencies. A later filtered command also installs a selected project that an earlier filtered install skipped [#11865](https://github.com/pnpm/pnpm/issues/11865).
