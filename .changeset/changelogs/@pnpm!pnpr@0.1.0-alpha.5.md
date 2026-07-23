## 0.1.0-alpha.5

### Patch Changes

- Fixed `catalog:` references in dependencies and overrides failing to resolve when installing through a pnpr server, which errored with "No catalog entry '<name>' was found for catalog 'default'." even though the catalog entry existed. Also fixed a crash on Windows when installing a nested workspace member (e.g. `packages/foo`) through a pnpr server [#13232](https://github.com/pnpm/pnpm/issues/13232).
