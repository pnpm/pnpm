## 1102.1.4

### Patch Changes

- `pnpm self-update` run in a project that pins pnpm through `packageManager` or `devEngines.packageManager` now also updates the global pnpm, as it does outside a project [#14747](https://github.com/pnpm/pnpm/issues/14747).

- `pnpm self-update` no longer leaves the previous pnpm in the global packages when it was installed as `@pnpm/exe`. `pnpm ls -g` now lists a single pnpm [#14709](https://github.com/pnpm/pnpm/issues/14709).

- `pnpm setup` no longer deletes aliases and other lines that sit between a `# pnpm` comment and the pnpm block in a shell startup file [#7067](https://github.com/pnpm/pnpm/issues/7067).

- On Nix, a dependency's bin named like a system utility such as `sed` can no longer redirect a POSIX bin shim or the `pnpm`, `pn`, `pnpx`, and `pnx` launchers. The shims and launchers now ignore `node_modules` and relative `PATH` entries while they locate their own files. Installing again replaces the shims already in `node_modules` [#14883](https://github.com/pnpm/pnpm/issues/14883).

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/building.policy@1100.1.3
  - @pnpm/cli.meta@1100.1.1
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.pick-registry-for-package@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/deps.security.signatures@1102.0.5
  - @pnpm/error@1100.2.0
  - @pnpm/global.commands@1102.0.3
  - @pnpm/global.packages@1101.1.4
  - @pnpm/installing.client@1100.3.11
  - @pnpm/installing.deps-restorer@1103.2.0
  - @pnpm/installing.env-installer@1103.0.6
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/store.connection-manager@1101.2.0
  - @pnpm/store.controller@1102.2.0
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
