## 1102.2.10

### Patch Changes

- With `injectWorkspacePackages: true`, a `workspace:` dependency written as a relative path, such as `workspace:../lib-a`, in a workspace package now resolves from that package's directory [#17284](https://github.com/pnpm/pnpm/issues/17284).

- An optional dependency written with a named-registry prefix, such as `work:^1.0.0`, now fails the install when it cannot be resolved but the lockfile already has a matching entry from that registry. Such a dependency was skipped, which removed its entries from the lockfile.

- `pnpm install` now resolves a scope's packages again when the scope is routed to another registry in `registries`. Before, the lockfile kept the versions and integrity resolved from the old registry, so the install failed when the new registry did not serve a locked version or served different content. pnpm records in the workspace state which registry each scope routed to, so a fresh clone without that record installs the lockfile as recorded. Changing the default registry resolves nothing again, and `pnpm install --frozen-lockfile` is unchanged.

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.5
  - @pnpm/config.pick-registry-for-package@1101.0.4
  - @pnpm/config.version-policy@1100.2.10
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/deps.graph-hasher@1100.3.9
  - @pnpm/deps.path@1101.0.8
  - @pnpm/fetching.pick-fetcher@1100.1.16
  - @pnpm/fs.symlink-dependency@1100.0.26
  - @pnpm/hooks.types@1101.0.7
  - @pnpm/lockfile.preferred-versions@1100.0.40
  - @pnpm/lockfile.pruner@1100.0.29
  - @pnpm/lockfile.types@1100.1.6
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/patching.config@1100.1.11
  - @pnpm/pkg-manifest.reader@1100.0.24
  - @pnpm/pkg-manifest.utils@1100.4.11
  - @pnpm/resolving.npm-resolver@1104.2.8
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/store.controller-types@1101.3.5
  - @pnpm/types@1102.1.3
