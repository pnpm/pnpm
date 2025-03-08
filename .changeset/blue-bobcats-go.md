---
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

`pnpm self-update` should always update the version in the `packageManager` field of `package.json`.
