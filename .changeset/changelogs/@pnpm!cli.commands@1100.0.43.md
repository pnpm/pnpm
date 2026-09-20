## 1100.0.43

### Patch Changes

- Fixed shell completion of package scripts for `pnpm run` and `pnpm run-script` [pnpm/pnpm#15034](https://github.com/pnpm/pnpm/issues/15034).

  Bash completion now preserves literal script names containing glob characters and shell punctuation in pnpm v11 and v12.

- Updated dependencies:
  - @pnpm/config.reader@1102.2.1
