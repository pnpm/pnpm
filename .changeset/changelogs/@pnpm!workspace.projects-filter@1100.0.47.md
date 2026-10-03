## 1100.0.47

### Patch Changes

- The `[<since>]` filter selector works again with Git 2.24 through 2.27 [#16561](https://github.com/pnpm/pnpm/issues/16561). With Git older than 2.24, the selector now fails with an error that names the required Git version.

- Updated dependencies:
  - @pnpm/config.matcher@1100.0.3
  - @pnpm/workspace.projects-graph@1100.0.42
  - @pnpm/workspace.projects-reader@1101.1.2
