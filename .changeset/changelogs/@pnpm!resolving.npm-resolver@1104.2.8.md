## 1104.2.8

### Patch Changes

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.5
  - @pnpm/config.pick-registry-for-package@1101.0.4
  - @pnpm/config.version-policy@1100.2.10
  - @pnpm/core-loggers@1101.0.4
  - @pnpm/deps.path@1101.0.8
  - @pnpm/pkg-manifest.utils@1100.4.11
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.25
  - @pnpm/resolving.registry.types@1100.2.5
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/resolving.tarball-url@1101.1.6
  - @pnpm/store.cafs@1100.3.9
  - @pnpm/types@1102.1.3
