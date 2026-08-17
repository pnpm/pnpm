## 1100.1.0

### Minor Changes

- A project that wasn't part of an install that moved a catalog entry now follows the entry the next time it is installed. It used to keep the version the entry resolved to before — a version the entry no longer allowed — and no later install corrected it, so one catalog entry ended up resolved to two versions.

### Patch Changes

- Updated dependencies:
  - @pnpm/installing.context@1100.1.2
  - @pnpm/lockfile.utils@1101.1.0
  - @pnpm/pkg-manifest.reader@1100.0.16
