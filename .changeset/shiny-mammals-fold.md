---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm link` should update overrides in `pnpm-workspace.yaml`, not in `package.json` [#9365](https://github.com/pnpm/pnpm/pull/9365).
