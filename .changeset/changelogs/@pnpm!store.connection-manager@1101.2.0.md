## 1101.2.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- `CreateNewStoreControllerOptions` is now exported from `@pnpm/store.connection-manager`. Store controllers now read custom certificate authorities from `cafile`.

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.1
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/installing.client@1100.3.11
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/store.controller@1102.2.0
  - @pnpm/store.index@1100.3.2
  - @pnpm/store.path@1100.0.8
  - @pnpm/types@1102.1.1
