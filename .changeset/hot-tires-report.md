---
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/plugin-commands-rebuild": patch
"@pnpm/plugin-commands-publishing": patch
"@pnpm/build-modules": patch
"@pnpm/config": patch
"pnpm": patch
---

Set the default `workspaceConcurrency` to `Math.min(os.availableParallelism(), 4)` [#9493](https://github.com/pnpm/pnpm/pull/9493).
