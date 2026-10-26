## 1100.3.27

### Patch Changes

- Output that fills the terminal, such as `pnpm run --stream` script output, is now written once. Before, every new line also rewrote all the lines already on screen [#17387](https://github.com/pnpm/pnpm/issues/17387).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.4
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.17
  - @pnpm/types@1102.1.3
