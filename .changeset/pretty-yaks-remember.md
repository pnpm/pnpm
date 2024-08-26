---
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

`pnpm deploy` should write the `node_modules/.modules.yaml` to the `node_modules` directory within the deploy directory [#7731](https://github.com/pnpm/pnpm/issues/7731).
