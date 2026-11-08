## 1100.0.43

### Patch Changes

- Relative local paths in root `overrides` now resolve from the workspace root when each project has its own lockfile [#17691](https://github.com/pnpm/pnpm/issues/17691).

  Installing a local directory dependency no longer fails when another project's install is writing a lockfile in that directory.

- Updated dependencies:
  - @pnpm/fs.packlist@1100.0.12
