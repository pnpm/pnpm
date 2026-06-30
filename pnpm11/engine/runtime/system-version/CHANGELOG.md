# @pnpm/engine.runtime.system-version

## 1100.0.3

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [681b593]
  - @pnpm/types@1101.3.2
  - @pnpm/cli.meta@1100.0.8

## 1100.0.2

### Patch Changes

- Updated dependencies [bf1b731]
  - @pnpm/types@1101.3.1
  - @pnpm/cli.meta@1100.0.7

## 1100.0.1

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/types@1101.3.0
  - @pnpm/cli.meta@1100.0.6

## 1100.0.0

### Minor Changes

- 35d2355: Validate `devEngines.runtime` and `engines.runtime` version ranges for `node`, `deno`, and `bun` when `onFail` is set to `error` or `warn`. Previously these settings only had an effect with `onFail: 'download'` — the `error` and `warn` modes silently did nothing [#11818](https://github.com/pnpm/pnpm/issues/11818). Violations now throw `ERR_PNPM_BAD_RUNTIME_VERSION`.

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0
  - @pnpm/cli.meta@1100.0.5
