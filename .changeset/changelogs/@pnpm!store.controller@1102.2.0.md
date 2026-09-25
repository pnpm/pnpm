## 1102.2.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/error@1100.2.0
  - @pnpm/fetching.fetcher-base@1100.2.11
  - @pnpm/hooks.types@1101.0.4
  - @pnpm/installing.package-requester@1102.2.0
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/store.cafs@1100.3.4
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/store.create-cafs-store@1100.0.31
  - @pnpm/store.index@1100.3.2
  - @pnpm/types@1102.1.1
