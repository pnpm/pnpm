---
"@pnpm/headless": patch
"pnpm": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-deploy": patch
---

Fix a regression in which `pnpm deploy` with `node-linker=hoisted` produces an empty `node_modules` directory [#6682](https://github.com/pnpm/pnpm/issues/6682).
