---
"pacquet": patch
---

`pnpm approve` without package arguments or `--all` now fails with `ERR_PNPM_APPROVE_NON_INTERACTIVE` when packages are awaiting approval in CI or without an interactive terminal.
