## 1103.0.14

### Patch Changes

- `pnpm install` and `pnpm rebuild` now refresh privately hoisted `.bin` shims in `node_modules/.pnpm/node_modules/.bin` after a dependency build replaces a script target with a native executable [#17402](https://github.com/pnpm/pnpm/issues/17402).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.40
  - @pnpm/config.reader@1102.3.8
  - @pnpm/deps.graph-hasher@1100.3.10
  - @pnpm/exec.lifecycle@1100.1.27
  - @pnpm/installing.context@1101.0.14
  - @pnpm/installing.deps-restorer@1103.2.8
  - @pnpm/lockfile.settings-checker@1100.2.16
  - @pnpm/lockfile.utils@1102.1.9
  - @pnpm/store.connection-manager@1101.3.7
