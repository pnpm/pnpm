## 1100.0.57

### Patch Changes

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- Updated dependencies:
  - @pnpm/config.pick-registry-for-package@1101.0.7
  - @pnpm/config.reader@1102.4.0
  - @pnpm/error@1100.2.3
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/types@1102.1.5
