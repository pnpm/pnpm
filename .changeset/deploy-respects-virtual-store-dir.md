---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now puts the virtual store at `virtualStoreDir`, resolved against the deploy directory. A shared-lockfile deploy records `virtualStoreDir` in the deployed `pnpm-workspace.yaml`. With the global virtual store enabled or an absolute `virtualStoreDir`, the deploy still uses `node_modules/.pnpm` [#8787](https://github.com/pnpm/pnpm/issues/8787).
