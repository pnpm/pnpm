# @pnpm/plugin-commands-config

## 1.0.2

### Patch Changes

- @pnpm/cli-utils@1.0.20
- @pnpm/config@16.2.2

## 1.0.1

### Patch Changes

- 83a627a25: The config command should work with the `--location=global` CLI option [#5841](https://github.com/pnpm/pnpm/issues/5841).
- Updated dependencies [d71dbf230]
  - @pnpm/config@16.2.1
  - @pnpm/cli-utils@1.0.19

## 1.0.0

### Major Changes

- 841f52e70: pnpm gets its own implementation of the following commands:

  - `pnpm config get`
  - `pnpm config set`
  - `pnpm config delete`
  - `pnpm config list`

  In previous versions these commands were passing through to npm CLI.

  PR: [#5829](https://github.com/pnpm/pnpm/pull/5829)
  Related issue: [#5621](https://github.com/pnpm/pnpm/issues/5621)

### Patch Changes

- Updated dependencies [841f52e70]
  - @pnpm/config@16.2.0
  - @pnpm/cli-utils@1.0.18
