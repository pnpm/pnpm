---
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

Fix an issue in which `pnpm deploy --prod` fails due to missing `devDependencies` [#8778](https://github.com/pnpm/pnpm/issues/8778).
