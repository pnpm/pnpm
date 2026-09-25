## 1100.2.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.1
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/engine.runtime.system-version@1100.0.12
  - @pnpm/error@1100.2.0
  - @pnpm/types@1102.1.1
