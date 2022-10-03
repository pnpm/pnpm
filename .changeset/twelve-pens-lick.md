---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm update --interactive` should not list dependencies ignored via the `pnpm.updateConfig.ignoreDependencies` setting.
