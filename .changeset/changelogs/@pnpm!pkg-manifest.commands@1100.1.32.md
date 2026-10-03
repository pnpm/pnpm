## 1100.1.32

### Patch Changes

- `pnpm -r pkg get` now reports every selected project when several share a package name. Projects with the same name are keyed by their directory relative to the workspace root. Before, only one of them appeared in the output.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.31
  - @pnpm/config.reader@1102.3.3
