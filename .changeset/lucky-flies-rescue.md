---
"@pnpm/tools.plugin-commands-self-updater": patch
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm self-update` should not read the pnpm settings from the `package.json` file in the current working directory.
