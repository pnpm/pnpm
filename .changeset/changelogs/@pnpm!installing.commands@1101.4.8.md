## 1101.4.8

### Patch Changes

- `pnpm install` now resolves a scope's packages again when the scope is routed to another registry in `registries`. Before, the lockfile kept the versions and integrity resolved from the old registry, so the install failed when the new registry did not serve a locked version or served different content. pnpm records in the workspace state which registry each scope routed to, so a fresh clone without that record installs the lockfile as recorded. Changing the default registry resolves nothing again, and `pnpm install --frozen-lockfile` is unchanged.

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- `pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.13
  - @pnpm/building.policy@1100.1.9
  - @pnpm/cli.utils@1101.0.34
  - @pnpm/config.normalize-registries@1101.0.5
  - @pnpm/config.pick-registry-for-package@1101.0.4
  - @pnpm/config.reader@1102.3.7
  - @pnpm/config.version-policy@1100.2.10
  - @pnpm/config.writer@1100.0.35
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/deps.github-actions@1100.1.18
  - @pnpm/deps.inspection.outdated@1100.1.39
  - @pnpm/deps.path@1101.0.8
  - @pnpm/deps.security.signatures@1102.0.11
  - @pnpm/deps.status@1100.1.32
  - @pnpm/global.commands@1102.0.10
  - @pnpm/global.packages@1101.1.9
  - @pnpm/hooks.pnpmfile@1100.0.38
  - @pnpm/installing.context@1101.0.13
  - @pnpm/installing.dedupe.check@1100.1.18
  - @pnpm/installing.deps-installer@1104.2.7
  - @pnpm/installing.env-installer@1103.0.13
  - @pnpm/installing.modules-yaml@1101.0.8
  - @pnpm/lockfile.fs@1100.2.15
  - @pnpm/lockfile.types@1100.1.6
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/network.auth-header@1101.1.18
  - @pnpm/network.fetch@1100.1.24
  - @pnpm/pkg-manifest.reader@1100.0.24
  - @pnpm/pkg-manifest.utils@1100.4.11
  - @pnpm/resolving.local-resolver@1101.2.8
  - @pnpm/resolving.npm-resolver@1104.2.8
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/store.connection-manager@1101.3.6
  - @pnpm/store.controller@1102.2.7
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.injected-deps-syncer@1100.0.46
  - @pnpm/workspace.project-manifest-reader@1100.1.5
  - @pnpm/workspace.project-manifest-writer@1100.0.22
  - @pnpm/workspace.projects-filter@1100.0.51
  - @pnpm/workspace.projects-graph@1100.0.46
  - @pnpm/workspace.projects-reader@1101.1.6
  - @pnpm/workspace.projects-sorter@1101.0.3
  - @pnpm/workspace.root-finder@1100.1.5
  - @pnpm/workspace.state@1100.0.52
  - @pnpm/workspace.task-scheduler@1100.0.6
  - @pnpm/workspace.workspace-manifest-reader@1100.3.1
  - @pnpm/workspace.workspace-manifest-writer@1100.2.9
