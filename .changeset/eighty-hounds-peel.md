---
"@pnpm/plugin-commands-deploy": patch
---

`pnpm deploy` should not touch the target directory if it already exists and isn't empty [#7351](https://github.com/pnpm/pnpm/issues/7351).

