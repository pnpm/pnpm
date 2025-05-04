---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/plugin-commands-releasing": patch
"@pnpm/build-modules": patch
"@pnpm/config": patch
---

Set the default `workspaceConcurrency` to `Math.min(os.availableParallelism(), 4)`
