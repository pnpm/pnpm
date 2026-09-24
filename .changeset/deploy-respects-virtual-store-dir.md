---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now puts the virtual store where `virtualStoreDir` says, resolved against the deploy directory, instead of always using `node_modules/.pnpm`. A shared-lockfile deploy also records the setting in the deployed `pnpm-workspace.yaml`, so a later install in the deploy directory keeps that layout. When the global virtual store is enabled, `virtualStoreDir` names the global store, so the deploy keeps the default `node_modules/.pnpm` [#8787](https://github.com/pnpm/pnpm/issues/8787).
