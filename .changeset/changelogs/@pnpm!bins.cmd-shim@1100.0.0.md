## 1100.0.0

### Patch Changes

- The `@zkochan/cmd-shim` package is now available as `@pnpm/bins.cmd-shim`.

- POSIX bin shims now convert a Windows-form path such as `C:\node_modules\.bin\tsc` correctly. The shim mangled the backslashes in such a path and could not reach the package it runs. Installing again replaces the shims already in `node_modules` [#14867](https://github.com/pnpm/pnpm/issues/14867).

- POSIX bin shims now take `cygpath` and `wslpath` from the system default path on Cygwin, MSYS2, and WSL2. The shims looked both helpers up on `PATH`, where a dependency's own bins come first, so a dependency could redirect another package's shim. Installing again replaces the shims already in `node_modules` [#14866](https://github.com/pnpm/pnpm/issues/14866).

- Updated dependencies:
  - @pnpm/fs.graceful-fs@1100.2.2
