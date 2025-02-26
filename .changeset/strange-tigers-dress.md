---
"@pnpm/plugin-commands-deploy": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/core": patch
pnpm: patch
---

Fix a bug causing entries in the `catalogs` section of the `pnpm-lock.yaml` file to be removed when `dedupe-peer-dependents=false` on a filtered install. [#9112](https://github.com/pnpm/pnpm/issues/9112)
