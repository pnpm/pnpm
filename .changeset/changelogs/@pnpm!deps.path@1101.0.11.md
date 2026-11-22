## 1101.0.11

### Patch Changes

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.10
  - @pnpm/types@1102.1.5
