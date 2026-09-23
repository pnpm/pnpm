---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

`pnpm root` now prints the configured `modulesDir`. It used to print `node_modules` regardless of the setting. A project's own `modulesDir` from `packageConfigs` is printed too [#9113](https://github.com/pnpm/pnpm/issues/9113).
