## 1100.0.42

### Patch Changes

- Installing no longer replaces the POSIX bin shims that a newer pnpm wrote in `node_modules/.bin`.

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build on macOS with the sandbox enabled. Installing again replaces the shims already in `node_modules` [#17750](https://github.com/pnpm/pnpm/issues/17750).

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- Updated dependencies:
  - @pnpm/bins.cmd-shim@1100.0.7
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/workspace.project-manifest-reader@1100.1.6
