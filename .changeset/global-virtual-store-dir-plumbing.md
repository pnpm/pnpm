---
"@pnpm/constants": patch
"@pnpm/config": patch
"@pnpm/core": patch
"@pnpm/config.deps-installer": patch
"@pnpm/package-store": patch
"@pnpm/store-connection-manager": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-self-updater": patch
"pnpm": patch
---

`globalVirtualStoreDir` is now threaded consistently through all runtime paths (install, config-deps installer, prune, self-update). When set explicitly it overrides the default `<storeDir>/links` path everywhere instead of only in the core install flow.
