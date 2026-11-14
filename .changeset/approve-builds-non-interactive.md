---
"@pnpm/building.commands": patch
"@pnpm/exec.commands": patch
"@pnpm/global.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm approve-builds` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal.
