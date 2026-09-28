## 1100.0.34

### Patch Changes

- Bin shims in `node_modules/.bin` run from Cygwin on Windows again. The shims passed a `/cygdrive/c/...` path to the Windows `node` found on `PATH`, so Node.js failed with `Cannot find module 'C:\cygdrive\c\...'` [#12845](https://github.com/pnpm/pnpm/issues/12845).

- On Windows, `pnpm env use -g` and `pnpm add -g node@runtime:<version>` now replace a `node.exe` in the global bin directory that is a broken symlink. Previously they failed with `ENOENT` [#5411](https://github.com/pnpm/pnpm/issues/5411).

- Files imported from the store now follow the umask of the install that writes them. Installing with a umask of `077` no longer leaves imported files readable by the group and others [#3807](https://github.com/pnpm/pnpm/issues/3807).

- Commands run from a POSIX shell through a dependency's own `node_modules/.bin`, such as `node_modules/vite/node_modules/.bin/esbuild`, no longer fail with `MODULE_NOT_FOUND` [#10189](https://github.com/pnpm/pnpm/issues/10189).

- `pnpm install` now links the executables of auto-installed peer dependencies into the workspace root's `node_modules/.bin`, including after a frozen-lockfile reinstall [#8511](https://github.com/pnpm/pnpm/issues/8511).

- `pnpm rebuild` with `nodeLinker: hoisted` no longer puts one package's parent `node_modules/.bin` directories on the `PATH` of the packages it builds after it.

- Updated dependencies:
  - @pnpm/bins.cmd-shim@1100.0.2
  - @pnpm/error@1100.2.1
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/workspace.project-manifest-reader@1100.1.1
