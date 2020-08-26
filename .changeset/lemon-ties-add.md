---
"@pnpm/plugin-commands-installation": patch
---

`pnpm install -r` should recreate the modules directory
if the hoisting patterns were updated in a local config file.
The hoisting patterns are configure via the `hoist-pattern`
and `public-hoist-pattern` settings.
