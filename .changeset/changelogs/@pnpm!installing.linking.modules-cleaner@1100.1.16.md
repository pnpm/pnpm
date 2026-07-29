## 1100.1.16

### Patch Changes

- Speed up installs after safe override changes by reusing unambiguous compatible dependency resolutions, pruning obsolete dependencies, applying independent replacements and removals together, and handling parent-scoped `"-"` overrides without full lockfile resolution.

- Updated dependencies:
  - @pnpm/bins.remover@1100.0.17
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.path@1100.0.12
  - @pnpm/lockfile.filtering@1100.2.0
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/store.controller-types@1100.1.11
  - @pnpm/types@1101.7.0
