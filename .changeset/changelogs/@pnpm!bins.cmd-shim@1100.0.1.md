## 1100.0.1

### Patch Changes

- `pnpm install` now links a dependency's bin even when the bin's file does not exist yet, such as a workspace package's bin that a build script creates after install. Previously pnpm printed a `Failed to create bin` warning. The command then stayed missing until `node_modules` was removed [pnpm/pnpm#10007](https://github.com/pnpm/pnpm/issues/10007), [pnpm/pnpm#10216](https://github.com/pnpm/pnpm/issues/10216).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- On Windows, bin shims run from Git Bash, MSYS2, or Cygwin now pass `NODE_PATH` to Node.js as Windows paths. A project installed from cmd or PowerShell gave its bins a `NODE_PATH` under the Git install directory when they ran from Git Bash. Installing again replaces the shims already in `node_modules` [#3360](https://github.com/pnpm/pnpm/issues/3360).

- On Nix, a dependency's bin named like a system utility such as `sed` can no longer redirect a POSIX bin shim or the `pnpm`, `pn`, `pnpx`, and `pnx` launchers. The shims and launchers now ignore `node_modules` and relative `PATH` entries while they locate their own files. Installing again replaces the shims already in `node_modules` [#14883](https://github.com/pnpm/pnpm/issues/14883).

- Updated dependencies:
  - @pnpm/fs.graceful-fs@1100.2.3
