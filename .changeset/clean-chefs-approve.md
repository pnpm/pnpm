---
"@pnpm/link-bins": patch
"pnpm": patch
---

Command shim should not set higher priority to the `node_modules/.pnpm/node_modules` directory through the `NODE_PATH` env variable, then the command's own `node_modules` directory [#5176](https://github.com/pnpm/pnpm/issues/5176).
