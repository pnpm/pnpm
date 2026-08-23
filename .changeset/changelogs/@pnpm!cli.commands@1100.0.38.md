## 1100.0.38

### Patch Changes

- Fixed workspace discovery for `pnpm-workspace.yaml` files without a `packages` field so commands only consider the workspace root instead of recursively scanning nested projects [#14047](https://github.com/pnpm/pnpm/issues/14047).

- Updated dependencies:
  - @pnpm/config.reader@1102.0.0
