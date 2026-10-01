## 1102.1.6

### Patch Changes

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build, where the system default path holds none of the utilities they call. Installing again replaces the shims already in `node_modules` [#16377](https://github.com/pnpm/pnpm/issues/16377).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.35
  - @pnpm/config.reader@1102.3.2
  - @pnpm/deps.security.signatures@1102.0.7
  - @pnpm/exec.npm-lifecycle@1100.0.3
  - @pnpm/global.commands@1102.0.5
  - @pnpm/global.packages@1101.1.6
  - @pnpm/installing.client@1100.3.13
  - @pnpm/installing.deps-restorer@1103.2.2
  - @pnpm/installing.env-installer@1103.0.8
  - @pnpm/lockfile.fs@1100.2.11
  - @pnpm/resolving.npm-resolver@1104.2.3
  - @pnpm/store.connection-manager@1101.3.1
  - @pnpm/store.controller@1102.2.2
