## 1104.2.7

### Patch Changes

- `pnpm install` now applies the `serverType` registry setting when it updates the lockfile for an override or catalog change without a full resolution. pnpm wrote the tarball URLs of a declared registry into the lockfile in that case.

- An install filtered to some workspace projects now fails if `node_modules` was installed with a different `nodeLinker`, `hoistPattern`, `publicHoistPattern`, store, or virtual store. Before, it recreated `node_modules` and left the projects it skipped without their dependencies. A `pnpm install` of the whole workspace still recreates `node_modules` [pnpm/tasks#217](https://github.com/pnpm/tasks/issues/217).

- `pnpm install` now resolves a scope's packages again when the scope is routed to another registry in `registries`. Before, the lockfile kept the versions and integrity resolved from the old registry, so the install failed when the new registry did not serve a locked version or served different content. pnpm records in the workspace state which registry each scope routed to, so a fresh clone without that record installs the lockfile as recorded. Changing the default registry resolves nothing again, and `pnpm install --frozen-lockfile` is unchanged.

- `pnpm install` now removes command shims from `node_modules/.bin` when a workspace dependency renames or removes an entry from its `bin` field [pnpm/tasks#211](https://github.com/pnpm/tasks/issues/211).

- `pnpm add`, `pnpm remove`, and `pnpm update` now fail when the `node-linker` setting differs from the one recorded in `node_modules`. A `pnpm install` of the whole workspace recreates `node_modules` with the new `node-linker`, and an install filtered to some projects fails [pnpm/tasks#203](https://github.com/pnpm/tasks/issues/203).

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.39
  - @pnpm/bins.remover@1100.0.30
  - @pnpm/building.after-install@1103.0.13
  - @pnpm/building.during-install@1102.2.10
  - @pnpm/building.policy@1100.1.9
  - @pnpm/config.normalize-registries@1101.0.5
  - @pnpm/config.package-is-installable@1100.2.5
  - @pnpm/config.version-policy@1100.2.10
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/deps.graph-hasher@1100.3.9
  - @pnpm/deps.path@1101.0.8
  - @pnpm/exec.lifecycle@1100.1.26
  - @pnpm/fs.symlink-dependency@1100.0.26
  - @pnpm/hooks.read-package-hook@1100.3.10
  - @pnpm/hooks.types@1101.0.7
  - @pnpm/installing.context@1101.0.13
  - @pnpm/installing.deps-resolver@1102.2.10
  - @pnpm/installing.deps-restorer@1103.2.7
  - @pnpm/installing.linking.direct-dep-linker@1100.0.26
  - @pnpm/installing.linking.hoist@1100.0.40
  - @pnpm/installing.linking.modules-cleaner@1100.1.31
  - @pnpm/installing.modules-yaml@1101.0.8
  - @pnpm/installing.package-requester@1102.2.7
  - @pnpm/lockfile.filtering@1100.2.14
  - @pnpm/lockfile.fs@1100.2.15
  - @pnpm/lockfile.preferred-versions@1100.0.40
  - @pnpm/lockfile.pruner@1100.0.29
  - @pnpm/lockfile.settings-checker@1100.2.15
  - @pnpm/lockfile.to-pnp@1101.0.12
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/lockfile.verification@1100.1.14
  - @pnpm/lockfile.walker@1100.0.29
  - @pnpm/network.auth-header@1101.1.18
  - @pnpm/patching.config@1100.1.11
  - @pnpm/pkg-manifest.utils@1100.4.11
  - @pnpm/pnpr.client@3.1.5
  - @pnpm/resolving.local-resolver@1101.2.8
  - @pnpm/resolving.npm-resolver@1104.2.8
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/store.controller-types@1101.3.5
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
