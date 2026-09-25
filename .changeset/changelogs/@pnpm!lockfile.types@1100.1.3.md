## 1100.1.3

### Patch Changes

- `pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).

- Merging lockfiles now preserves recorded configuration fields such as `overrides`, `neverBuiltDependencies`, `patchedDependencies`, `packageExtensionsChecksum`, `settings`, and `catalogs` [pnpm/pnpm#8366](https://github.com/pnpm/pnpm/issues/8366).

- Updated dependencies:
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/types@1102.1.1
