---
"@pnpm/plugin-commands-env": patch
"pnpm": patch
---

`pnpm env use` should retry deleting the previous node.js executable [#6587](https://github.com/pnpm/pnpm/issues/6587).
