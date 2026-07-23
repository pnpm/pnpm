## 1100.0.33

### Patch Changes

- `@pnpm/modules-mounter.daemon` now ships the `lib/index.js` entry point its manifest declares; importing the package no longer fails to resolve.

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).

- Updated dependencies:
  - @pnpm/config.reader@1101.14.0
  - @pnpm/deps.path@1100.0.11
  - @pnpm/lockfile.fs@1100.1.14
  - @pnpm/lockfile.utils@1100.1.5
  - @pnpm/store.cafs@1100.1.15
  - @pnpm/store.index@1100.2.2
  - @pnpm/store.path@1100.0.3
  - @pnpm/types@1101.6.0
