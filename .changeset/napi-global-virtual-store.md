---
"@pnpm/napi": minor
---

`install` accepts `enableGlobalVirtualStore`, `globalVirtualStoreDir`, `packageExtensions` and `patchedDependencies`, and `readConfig` reports `enableGlobalVirtualStore`, `globalVirtualStoreDir`, `virtualStoreDir` and `effectiveVirtualStoreDir`. Hosts embedding the engine can now use the global virtual store, declare dependencies a package failed to declare, patch a package, and locate the virtual store instead of assuming `node_modules/.pnpm`.
