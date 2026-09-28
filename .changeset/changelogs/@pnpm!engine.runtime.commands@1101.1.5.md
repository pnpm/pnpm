## 1101.1.5

### Patch Changes

- `pnpm env remove --global` deletes Node.js versions that pnpm installed into its own store, including when another tool installed pnpm [pnpm/pnpm#8357](https://github.com/pnpm/pnpm/issues/8357).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/engine.runtime.node-resolver@1101.3.4
  - @pnpm/error@1100.2.1
  - @pnpm/global.commands@1102.0.4
  - @pnpm/global.packages@1101.1.5
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/network.fetch@1100.1.19
