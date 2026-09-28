## 1100.0.46

### Patch Changes

- With `nodeLinker: hoisted`, `pnpm install` now restores a workspace project's `node_modules` after it was deleted. Before, the install printed "Already up to date" and left the project without the dependencies nested under it.

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- On Windows, pnpm now retries writing the workspace state file while another process, such as an antivirus scanner, briefly holds it open [#14550](https://github.com/pnpm/pnpm/issues/14550).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.1
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
