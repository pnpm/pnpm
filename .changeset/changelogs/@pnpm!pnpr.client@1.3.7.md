## 1.3.7

### Patch Changes

- Fixed `catalog:` references in dependencies and overrides failing to resolve when installing through a pnpr server, which errored with "No catalog entry '<name>' was found for catalog 'default'." even though the catalog entry existed. Also fixed a crash on Windows when installing a nested workspace member (e.g. `packages/foo`) through a pnpr server [#13232](https://github.com/pnpm/pnpm/issues/13232).

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).

- Updated dependencies:
  - @pnpm/catalogs.types@1100.0.1
  - @pnpm/lockfile.fs@1100.1.14
  - @pnpm/lockfile.types@1100.0.16
