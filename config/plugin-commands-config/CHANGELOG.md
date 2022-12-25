# @pnpm/plugin-commands-config

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
