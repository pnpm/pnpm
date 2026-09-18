---
"pacquet": patch
---

`pnpm peers check` now loads the pnpmfile, so catalogs added by an `updateConfig` hook resolve the `catalog:` peer dependencies of linked workspace packages [pnpm/pnpm#15047](https://github.com/pnpm/pnpm/issues/15047).
