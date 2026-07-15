## 1100.3.8

### Patch Changes

- Keep the interactive `minimumReleaseAge` approval prompt visible during `pnpm install`. The progress reporter now pauses its redraws while a prompt is waiting for input instead of overwriting it, so the install no longer hangs on a question the user cannot see [#13019](https://github.com/pnpm/pnpm/issues/13019).

- Updated dependencies:
  - @pnpm/config.reader@1101.12.1
  - @pnpm/core-loggers@1100.2.3
