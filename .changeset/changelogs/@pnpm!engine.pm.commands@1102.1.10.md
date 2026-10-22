## 1102.1.10

### Patch Changes

- `pnpm self-update` now updates the project's pnpm pin when Homebrew installed pnpm or Corepack runs it. The global pnpm is left alone [#17001](https://github.com/pnpm/pnpm/issues/17001).

- `pnpm setup` now completes when Windows registry environment values cannot be read or written. It prints PowerShell commands to configure `PNPM_HOME` and `PATH` without registry access [pnpm/pnpm#17167](https://github.com/pnpm/pnpm/issues/17167).

- `pnpm setup` now succeeds when it cannot tell which shell configuration file to update, for example in a container where `SHELL` is not set. It installs pnpm and prints the `PNPM_HOME` and `PATH` lines to add yourself. Previously it failed with `ERR_PNPM_UNKNOWN_SHELL`, and so did the installer script at get.pnpm.io [#16973](https://github.com/pnpm/pnpm/issues/16973).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.38
  - @pnpm/building.during-install@1102.2.9
  - @pnpm/building.policy@1100.1.8
  - @pnpm/cli.meta@1100.1.3
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.pick-registry-for-package@1101.0.3
  - @pnpm/config.reader@1102.3.6
  - @pnpm/config.version-policy@1100.2.9
  - @pnpm/deps.graph-hasher@1100.3.8
  - @pnpm/deps.security.signatures@1102.0.10
  - @pnpm/exec.npm-lifecycle@1100.0.6
  - @pnpm/global.commands@1102.0.9
  - @pnpm/global.packages@1101.1.8
  - @pnpm/installing.client@1100.3.17
  - @pnpm/installing.deps-restorer@1103.2.6
  - @pnpm/installing.env-installer@1103.0.12
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/pkg-manifest.utils@1100.4.10
  - @pnpm/resolving.npm-resolver@1104.2.7
  - @pnpm/store.connection-manager@1101.3.5
  - @pnpm/store.controller@1102.2.6
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.project-manifest-reader@1100.1.4
