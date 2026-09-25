## 1100.0.33

### Patch Changes

- Binary shims now correctly resolve dependency paths when `modules-dir` is customized [#3604](https://github.com/pnpm/pnpm/issues/3604).

- With `nodeLinker: hoisted`, `hoistWorkspacePackages` now links each workspace project that `hoistPattern` or `publicHoistPattern` selects into the root `node_modules`, unless a hoisted package or a root dependency already uses its name. The project's bins are linked into the root `node_modules/.bin` [#7553](https://github.com/pnpm/pnpm/issues/7553).

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).

- `pnpm install` now links a dependency's bin even when the bin's file does not exist yet, such as a workspace package's bin that a build script creates after install. Previously pnpm printed a `Failed to create bin` warning. The command then stayed missing until `node_modules` was removed [pnpm/pnpm#10007](https://github.com/pnpm/pnpm/issues/10007), [pnpm/pnpm#10216](https://github.com/pnpm/pnpm/issues/10216).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- Executable linking now checks for all executable permission bits before skipping chmod. Targets with partial execute permissions previously skipped permission repair and remained non-executable for other users [pnpm/pnpm#3699](https://github.com/pnpm/pnpm/issues/3699).

- On Nix, a dependency's bin named like a system utility such as `sed` can no longer redirect a POSIX bin shim or the `pnpm`, `pn`, `pnpx`, and `pnx` launchers. The shims and launchers now ignore `node_modules` and relative `PATH` entries while they locate their own files. Installing again replaces the shims already in `node_modules` [#14883](https://github.com/pnpm/pnpm/issues/14883).

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- Bin linking leaves workspace and linked dependency files outside node_modules unchanged. Already executable bin files no longer receive redundant permission changes.

- Updated dependencies:
  - @pnpm/bins.cmd-shim@1100.0.1
  - @pnpm/bins.resolver@1100.0.17
  - @pnpm/error@1100.2.0
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
