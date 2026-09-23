---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

`pnpm root` now prints the configured `modulesDir`, including one a `packageConfigs` entry gives the project, instead of always printing `node_modules` [#9113](https://github.com/pnpm/pnpm/issues/9113).
