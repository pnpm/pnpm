## 12.0.0-rc.0

### Minor Changes

- `install` accepts `enableGlobalVirtualStore`, `globalVirtualStoreDir`, `packageExtensions` and `patchedDependencies`, and `readConfig` reports `enableGlobalVirtualStore`, `globalVirtualStoreDir`, `virtualStoreDir` and `effectiveVirtualStoreDir`. Hosts embedding the engine can now use the global virtual store, declare dependencies a package failed to declare, patch a package, and locate the virtual store instead of assuming `node_modules/.pnpm`.

### Patch Changes

- Fixed `resolveDependency({ fullMetadata: true })` returning a manifest stripped down to the abbreviated npm field set. Registry-custom fields on the version object (such as Bit's `componentId`) are now preserved.
