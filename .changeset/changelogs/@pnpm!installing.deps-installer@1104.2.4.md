## 1104.2.4

### Patch Changes

- `WantedDependency.alias` is now optional. `getWantedDependencies` returns `ManifestWantedDependency`, whose `alias` is required. Use `hasAlias` to narrow a `WantedDependency` to it.

- `pnpm add` and `pnpm install` now keep the peer dependencies that `pnpm-lock.yaml` records for a package they did not update. A registry whose metadata disagrees with the package's `package.json`, for example by omitting `peerDependenciesMeta`, made `pnpm add` and `pnpm dedupe` write different lockfiles, so `pnpm dedupe --check` failed after `pnpm add` [#16615](https://github.com/pnpm/pnpm/issues/16615).

- Lockfile verification now checks the tarballs inside a `variations` resolution against the registry. A `name@version` lockfile entry with an empty `variations` resolution is now rejected.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.37
  - @pnpm/bins.remover@1100.0.28
  - @pnpm/building.after-install@1103.0.10
  - @pnpm/building.during-install@1102.2.7
  - @pnpm/building.policy@1100.1.6
  - @pnpm/catalogs.config@1100.0.10
  - @pnpm/catalogs.resolver@1100.1.2
  - @pnpm/config.normalize-registries@1101.0.3
  - @pnpm/config.package-is-installable@1100.2.3
  - @pnpm/config.parse-overrides@1100.1.8
  - @pnpm/config.version-policy@1100.2.7
  - @pnpm/constants@1102.0.1
  - @pnpm/crypto.hash@1100.0.8
  - @pnpm/deps.graph-hasher@1100.3.7
  - @pnpm/deps.path@1101.0.6
  - @pnpm/error@1100.2.2
  - @pnpm/exec.lifecycle@1100.1.23
  - @pnpm/fs.read-modules-dir@1100.0.4
  - @pnpm/fs.symlink-dependency@1100.0.24
  - @pnpm/hooks.read-package-hook@1100.3.7
  - @pnpm/installing.context@1101.0.10
  - @pnpm/installing.deps-resolver@1102.2.7
  - @pnpm/installing.deps-restorer@1103.2.4
  - @pnpm/installing.linking.direct-dep-linker@1100.0.24
  - @pnpm/installing.linking.hoist@1100.0.37
  - @pnpm/installing.linking.modules-cleaner@1100.1.29
  - @pnpm/installing.modules-yaml@1101.0.6
  - @pnpm/installing.package-requester@1102.2.4
  - @pnpm/lockfile.filtering@1100.2.12
  - @pnpm/lockfile.fs@1100.2.13
  - @pnpm/lockfile.preferred-versions@1100.0.38
  - @pnpm/lockfile.pruner@1100.0.27
  - @pnpm/lockfile.settings-checker@1100.2.12
  - @pnpm/lockfile.to-pnp@1101.0.10
  - @pnpm/lockfile.utils@1102.1.6
  - @pnpm/lockfile.verification@1100.1.11
  - @pnpm/lockfile.walker@1100.0.27
  - @pnpm/network.auth-header@1101.1.16
  - @pnpm/patching.config@1100.1.9
  - @pnpm/pkg-manifest.utils@1100.4.9
  - @pnpm/pnpr.client@3.1.3
  - @pnpm/resolving.local-resolver@1101.2.6
  - @pnpm/resolving.npm-resolver@1104.2.5
  - @pnpm/store.index@1101.0.1
  - @pnpm/workspace.project-manifest-reader@1100.1.3
