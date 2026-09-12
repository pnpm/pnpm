---
"@pnpm/engine.pm.commands": patch
"@pnpm/cli.default-reporter": patch
"@pnpm/cli.meta": minor
"pnpm": patch
"pacquet": patch
---

The update notification now suggests `pnpm self-update` when `PNPM_HOME` manages the pnpm in use, and the [standalone install script](https://pnpm.io/installation) otherwise — under Corepack, or when another package manager installed pnpm. `pnpm self-update` under Corepack names the standalone install script too.
