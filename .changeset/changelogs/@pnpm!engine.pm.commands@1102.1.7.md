## 1102.1.7

### Patch Changes

- `pnpm self-update` now fails for Homebrew-installed pnpm and prints the `brew upgrade` command for the installed formula, such as `brew upgrade pnpm` or `brew upgrade pnpm@11`. It used to install a second copy of pnpm that the Homebrew one kept shadowing [#16547](https://github.com/pnpm/pnpm/issues/16547).

- A `devEngines.packageManager` range now records the running pnpm in `pnpm-lock.yaml` only if it meets `minimumReleaseAge`. Otherwise pnpm records the newest version in the range that meets it. If no version in the range does, pnpm still records the running pnpm [#16431](https://github.com/pnpm/pnpm/issues/16431).

- On Windows, `pnpm self-update` now replaces a `pnpm.exe` left in `PNPM_HOME` or in `PNPM_HOME\bin`. In `PNPM_HOME`, that executable kept running the old version after a successful update. In `PNPM_HOME\bin`, the update failed with `EPERM`. If the executable was in `PNPM_HOME`, `self-update` now asks you to run `pnpm setup` [#9094](https://github.com/pnpm/pnpm/issues/9094).

- pnpm can now switch to a `packageManager` version below 11 on x64 musl Linux, such as Alpine [#16467](https://github.com/pnpm/pnpm/issues/16467).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.36
  - @pnpm/building.policy@1100.1.5
  - @pnpm/cli.meta@1100.1.2
  - @pnpm/cli.utils@1101.0.31
  - @pnpm/config.reader@1102.3.3
  - @pnpm/config.version-policy@1100.2.6
  - @pnpm/deps.graph-hasher@1100.3.6
  - @pnpm/deps.security.signatures@1102.0.8
  - @pnpm/exec.npm-lifecycle@1100.0.4
  - @pnpm/global.commands@1102.0.6
  - @pnpm/installing.client@1100.3.14
  - @pnpm/installing.deps-restorer@1103.2.3
  - @pnpm/installing.env-installer@1103.0.9
  - @pnpm/lockfile.fs@1100.2.12
  - @pnpm/pkg-manifest.utils@1100.4.8
  - @pnpm/resolving.npm-resolver@1104.2.4
  - @pnpm/store.connection-manager@1101.3.2
  - @pnpm/store.controller@1102.2.3
  - @pnpm/workspace.project-manifest-reader@1100.1.2
