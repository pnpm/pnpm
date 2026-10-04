## 1100.3.23

### Patch Changes

- `pnpm install` now prints a warning with the error when an optional dependency cannot be fetched and is skipped. The skipped package is no longer linked into `node_modules` as a broken symlink or listed among the added dependencies. The `pnpm:skipped-optional-dependency` log reports the skip with the `fetch_failure` reason [#16514](https://github.com/pnpm/pnpm/issues/16514).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.2
  - @pnpm/core-loggers@1101.0.2
