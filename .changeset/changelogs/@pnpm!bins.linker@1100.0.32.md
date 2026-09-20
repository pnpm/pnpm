## 1100.0.32

### Patch Changes

- The `@zkochan/cmd-shim` package is now available as `@pnpm/bins.cmd-shim`.

- `pnpm add -g` and `pnpm update -g` now ignore incomplete unrelated global package groups when every command from the replaced group is retained. Operations that could remove a global command still require complete ownership information.

- A dependency's own bins can no longer take over another package's bin shim. The POSIX shims pnpm generates used to look up their shell helpers on `PATH`, where a dependency's bins come first [#14837](https://github.com/pnpm/pnpm/issues/14837). Reinstalling replaces the shims already in your `node_modules`. On Cygwin, MSYS2, and WSL the shims still take their Windows path conversion from `PATH`, so a dependency can still redirect them there.

- POSIX bin shims now convert a Windows-form path such as `C:\node_modules\.bin\tsc` correctly. The shim mangled the backslashes in such a path and could not reach the package it runs. Installing again replaces the shims already in `node_modules` [#14867](https://github.com/pnpm/pnpm/issues/14867).

- POSIX bin shims now take `cygpath` and `wslpath` from the system default path on Cygwin, MSYS2, and WSL2. The shims looked both helpers up on `PATH`, where a dependency's own bins come first, so a dependency could redirect another package's shim. Installing again replaces the shims already in `node_modules` [#14866](https://github.com/pnpm/pnpm/issues/14866).

- Updated dependencies:
  - @pnpm/pkg-manifest.utils@1100.4.5
  - @pnpm/workspace.project-manifest-reader@1100.0.29
