## 1102.1.0

### Minor Changes

- `pnpm init` now pins the latest pnpm version, instead of the version of pnpm that ran the command. A project scaffolded by an outdated pnpm therefore no longer inherits that staleness through its own `devEngines.packageManager` / `packageManager` pin [#7490](https://github.com/pnpm/pnpm/issues/7490).

  The version is read from the `latest` tag on the package-manager registries. When that lookup cannot answer — no network, an unreachable or slow registry, `offline`, or a `latest` that the `minimumReleaseAge` / `trustPolicy` settings reject — `pnpm init` pins the running version as before, and never fails or hangs on the lookup. A `latest` that is older than the running pnpm is never pinned either.

### Patch Changes

- The update notification now suggests `pnpm self-update` when `PNPM_HOME` manages the pnpm in use, and the [standalone install script](https://pnpm.io/installation) otherwise — under Corepack, or when another package manager installed pnpm. `pnpm self-update` under Corepack names the standalone install script too.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.29
  - @pnpm/building.policy@1100.0.21
  - @pnpm/cli.meta@1100.1.0
  - @pnpm/cli.utils@1101.0.25
  - @pnpm/config.pick-registry-for-package@1101.0.1
  - @pnpm/config.reader@1102.1.0
  - @pnpm/config.version-policy@1100.2.2
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/deps.security.signatures@1102.0.1
  - @pnpm/global.commands@1101.1.0
  - @pnpm/global.packages@1101.1.0
  - @pnpm/installing.client@1100.3.7
  - @pnpm/installing.deps-restorer@1103.1.0
  - @pnpm/installing.env-installer@1103.0.2
  - @pnpm/lockfile.fs@1100.2.5
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/network.auth-header@1101.1.12
  - @pnpm/pkg-manifest.utils@1100.4.2
  - @pnpm/resolving.npm-resolver@1104.1.0
  - @pnpm/store.connection-manager@1101.1.0
  - @pnpm/store.controller@1102.1.0
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.project-manifest-reader@1100.0.26
