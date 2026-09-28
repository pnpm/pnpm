## 1100.0.2

### Patch Changes

- On Windows, the `.cmd` command shims in `node_modules/.bin` now keep a `%` in the project path. Before, cmd.exe expanded it as a variable reference, so the command received a mangled `NODE_PATH` [#15716](https://github.com/pnpm/pnpm/issues/15716).

- Bin shims in `node_modules/.bin` run from Cygwin on Windows again. The shims passed a `/cygdrive/c/...` path to the Windows `node` found on `PATH`, so Node.js failed with `Cannot find module 'C:\cygdrive\c\...'` [#12845](https://github.com/pnpm/pnpm/issues/12845).

- Fixed Windows command shims failing to run tools whose paths contain non-ASCII characters [#6999](https://github.com/pnpm/pnpm/issues/6999).

- PowerShell command shims now run tools whose paths contain non-ASCII characters in Windows PowerShell 5.1 [#16217](https://github.com/pnpm/pnpm/issues/16217).

- Commands run from a POSIX shell through a dependency's own `node_modules/.bin`, such as `node_modules/vite/node_modules/.bin/esbuild`, no longer fail with `MODULE_NOT_FOUND` [#10189](https://github.com/pnpm/pnpm/issues/10189).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
