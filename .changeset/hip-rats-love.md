---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm remove` should not link dependencies from the workspace, when `link-workspace-packages` is set to `false` [#7674](https://github.com/pnpm/pnpm/issues/7674).
