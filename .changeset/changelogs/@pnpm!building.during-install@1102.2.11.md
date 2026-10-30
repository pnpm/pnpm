## 1102.2.11

### Patch Changes

- `pnpm install` and `pnpm rebuild` now refresh privately hoisted `.bin` shims in `node_modules/.pnpm/node_modules/.bin` after a dependency build replaces a script target with a native executable [#17402](https://github.com/pnpm/pnpm/issues/17402).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.40
  - @pnpm/config.reader@1102.3.8
  - @pnpm/deps.graph-hasher@1100.3.10
  - @pnpm/exec.lifecycle@1100.1.27
  - @pnpm/pnpr.client@3.1.6
