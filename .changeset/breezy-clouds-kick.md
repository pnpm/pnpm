---
"@pnpm/plugin-commands-config": patch
"pnpm": patch
---

`pnpm config set` should convert the settings to their correct type before adding them to `pnpm-workspace.yaml` [#9355](https://github.com/pnpm/pnpm/issues/9355).
