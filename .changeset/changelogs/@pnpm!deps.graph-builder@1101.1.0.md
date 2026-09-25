## 1101.1.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.package-is-installable@1100.2.0
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/deps.path@1101.0.4
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/hooks.types@1101.0.4
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/patching.config@1100.1.7
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
