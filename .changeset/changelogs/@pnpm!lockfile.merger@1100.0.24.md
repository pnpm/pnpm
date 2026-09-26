## 1100.0.24

### Patch Changes

- `pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).

- Merging lockfiles now preserves recorded configuration fields such as `overrides`, `neverBuiltDependencies`, `patchedDependencies`, `packageExtensionsChecksum`, `settings`, and `catalogs` [pnpm/pnpm#8366](https://github.com/pnpm/pnpm/issues/8366).

- Updated dependencies:
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/types@1102.1.1
