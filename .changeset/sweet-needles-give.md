---
"@pnpm/build-modules": minor
"@pnpm/config": minor
"@pnpm/core": minor
"@pnpm/headless": minor
"@pnpm/lifecycle": minor
"@pnpm/plugin-commands-installation": minor
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/plugin-commands-script-runners": minor
---

New setting added: `scriptsPrependNodePath`. This setting can be `true`, `false`, or `warn-only`.
When `true`, the path to the `node` executable with which pnpm executed is prepended to the `PATH` of the scripts.
When `warn-only`, pnpm will print a warning if the scripts run with a `node` binary that differs from the `node` binary executing the pnpm CLI.
