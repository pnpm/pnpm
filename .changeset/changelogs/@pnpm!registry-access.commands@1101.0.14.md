## 1101.0.14

### Patch Changes

- When the version or range given to `pnpm unpublish` matches every published version, the error now says so and shows the same command with `--force`. It suggested running `pnpm unpublish <pkg>@<version>`, the command that had just failed.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.8
