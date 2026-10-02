---
"@pnpm/exec.commands": patch
"pnpm": patch
---

`pnpm run -r` now closes the collapsible CI log section of a project whose script fails, so the output of later projects is no longer nested inside it. `--resume-from` no longer crashes when a saved run state file contains `null`.
