## 1100.0.45

### Patch Changes

- `pnpm install` no longer fails when writing the workspace state file encounters an error. Failures to update the state file now emit a warning instead of aborting the install [pnpm/pnpm#14550](https://github.com/pnpm/pnpm/issues/14550).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.0
  - @pnpm/types@1102.1.1
