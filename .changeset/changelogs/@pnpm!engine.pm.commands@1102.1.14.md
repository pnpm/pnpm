## 1102.1.14

### Patch Changes

- `pnpm self-update` now updates the project's `engines.pnpm` range along with its pnpm pin, keeping the `^` or `~` operator [pnpm/pnpm#17732](https://github.com/pnpm/pnpm/issues/17732).

- Concurrent installs into a global virtual store no longer wait on each other. A package that needs a build is built in a private directory and moved into the store once its build has finished, so another project never links a half-built package. An install with `--ignore-scripts` links the packages it does not build from their own store directories, and `pnpm store prune` removes the build directories of installs that were interrupted [#15568](https://github.com/pnpm/pnpm/issues/15568).

- `pnpm setup` and the install script now support PowerShell as the login shell on Linux and macOS. They add `PNPM_HOME` to `PATH` in `profile.ps1` under `$XDG_CONFIG_HOME/powershell` or `~/.config/powershell` [#18026](https://github.com/pnpm/pnpm/issues/18026).

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build on macOS with the sandbox enabled. Installing again replaces the shims already in `node_modules` [#17750](https://github.com/pnpm/pnpm/issues/17750).

- `pnpm setup` rejects a `PNPM_HOME` value that would split the saved `PATH` [pnpm/tasks#315](https://github.com/pnpm/tasks/issues/315).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.42
  - @pnpm/building.during-install@1102.2.13
  - @pnpm/building.policy@1100.1.10
  - @pnpm/cli.utils@1101.0.35
  - @pnpm/config.reader@1102.3.9
  - @pnpm/deps.graph-hasher@1100.3.12
  - @pnpm/deps.security.signatures@1102.0.12
  - @pnpm/exec.npm-lifecycle@1100.0.8
  - @pnpm/fs.graceful-fs@1100.2.6
  - @pnpm/global.commands@1102.0.13
  - @pnpm/global.packages@1101.1.11
  - @pnpm/installing.client@1100.3.21
  - @pnpm/installing.deps-restorer@1103.2.10
  - @pnpm/installing.env-installer@1103.0.16
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/os.env.path-extender@1100.0.2
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/resolving.npm-resolver@1104.2.10
  - @pnpm/store.connection-manager@1101.3.9
  - @pnpm/store.controller@1102.2.10
  - @pnpm/workspace.project-manifest-reader@1100.1.6
