## 1101.3.0

### Minor Changes

- `pnpm setup` now appends `PNPM_HOME` and the global bin directory to the GitHub Actions environment files (`GITHUB_ENV` and `GITHUB_PATH`), so later steps in the same job can run `pnpm add --global` and other global commands [#9191](https://github.com/pnpm/pnpm/issues/9191).

- `pnpm self-update` no longer takes any instruction from the project it is run in:

  - pnpm is fetched through the same trusted registry and auth configuration used when switching pnpm versions, so a project `.npmrc` or `pnpm-workspace.yaml` can no longer redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is no longer loaded. Pnpmfiles from trusted sources (the `pnpmfile` setting, the global pnpmfile, config dependencies) still apply.
  - The `minimumReleaseAge` settings in `pnpm-workspace.yaml` no longer affect `self-update`. They still govern the project's own dependencies; for `self-update` the cooldown now comes from the built-in default, your global config, a `PNPM_CONFIG_*` environment variable, or a command-line flag. This fixes `self-update` failing inside a workspace that raises the cutoff while succeeding everywhere else, and stops a repository from either waiving the cooldown or keeping you on an outdated pnpm by raising it.
  - The same applies to the `trustPolicy` settings and to `ci`: a project can no longer weaken the trust check that guards the pnpm download, nor re-enable the confirmation prompt that a CI run suppresses.

  When `self-update` refuses a version that is younger than the cutoff, an interactive run now offers to update anyway; non-interactive runs still fail. CI never prompts, even on a runner that attaches a TTY.

### Patch Changes

- Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

  Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.23
  - @pnpm/building.policy@1100.0.16
  - @pnpm/cli.meta@1100.0.12
  - @pnpm/cli.utils@1101.0.20
  - @pnpm/config.pick-registry-for-package@1100.0.13
  - @pnpm/config.reader@1101.15.0
  - @pnpm/config.version-policy@1100.1.10
  - @pnpm/deps.graph-hasher@1100.2.13
  - @pnpm/deps.security.signatures@1101.2.8
  - @pnpm/global.commands@1100.0.41
  - @pnpm/global.packages@1100.0.14
  - @pnpm/installing.client@1100.3.0
  - @pnpm/installing.deps-restorer@1102.2.0
  - @pnpm/installing.env-installer@1102.0.13
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/network.auth-header@1101.1.7
  - @pnpm/resolving.npm-resolver@1102.1.9
  - @pnpm/store.connection-manager@1100.3.13
  - @pnpm/store.controller@1102.0.9
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.project-manifest-reader@1100.0.21
