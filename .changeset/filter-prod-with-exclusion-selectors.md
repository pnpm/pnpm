---
"@pnpm/exec.commands": patch
"@pnpm/workspace.projects-filter": patch
"pnpm": patch
"pacquet": patch
---

Combining `--filter` and `--filter-prod` with exclusion selectors such as `!pkg` now evaluates all selectors together in command-line order. An exclusion passed on one flag while inclusions were passed on the other no longer selects every workspace project.
