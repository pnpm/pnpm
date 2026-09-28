## 1101.2.5

### Patch Changes

- Fixed `pnpm rebuild` modifying packages shared with projects that have not approved their build scripts when using the global virtual store.

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.7
  - @pnpm/building.policy@1100.1.4
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/config.writer@1100.0.29
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/global.packages@1101.1.5
  - @pnpm/installing.commands@1101.4.1
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/store.connection-manager@1101.3.0
  - @pnpm/workspace.task-scheduler@1100.0.3
  - @pnpm/workspace.workspace-manifest-reader@1100.2.1
