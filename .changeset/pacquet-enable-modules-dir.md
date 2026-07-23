---
"pacquet": minor
---

The `enableModulesDir: false` setting is now honored: the install resolves and writes `pnpm-lock.yaml` but creates no `node_modules` directory (unless the global virtual store is enabled, in which case packages are still materialized into the store).
