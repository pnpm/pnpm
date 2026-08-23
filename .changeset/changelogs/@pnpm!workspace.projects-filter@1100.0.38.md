## 1100.0.38

### Patch Changes

- Fixed workspace discovery for `pnpm-workspace.yaml` files without a `packages` field so commands only consider the workspace root instead of recursively scanning nested projects [#14047](https://github.com/pnpm/pnpm/issues/14047).

- Updated dependencies:
  - @pnpm/error@1100.1.3
  - @pnpm/workspace.projects-graph@1100.0.34
  - @pnpm/workspace.projects-reader@1101.0.24
  - @pnpm/workspace.workspace-manifest-reader@1100.1.7
