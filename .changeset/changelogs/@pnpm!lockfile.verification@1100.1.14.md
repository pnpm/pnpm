## 1100.1.14

### Patch Changes

- Sped up installs of projects whose local directory dependencies share transitive local directory dependencies. pnpm no longer checks a shared directory again for each direct dependency that reaches it [#17374](https://github.com/pnpm/pnpm/issues/17374).

- A package that a plain range and a `<prefix>:` specifier both resolve from one registry is now recorded once in `pnpm-lock.yaml` and installed once. The lockfile key names the registry a package resolved from. When that registry declares a prefix, through `prefix` in `registries` or through `namedRegistries`, the package is keyed `<name>@<prefix>:<version>` however its dependency was written: with the prefix, as a plain range its scope routes to that registry, or through the default registry. Before, the plain range was keyed `<name>@<version>`, so one tarball was installed twice and `instanceof` checks and module singletons broke between the two copies. The same name and version served by two registries still get separate entries. When several prefixes declare one registry, its packages are keyed by the lexicographically smallest prefix.

  If a scope you route reaches a registry that also declares a prefix, your next `pnpm install` re-keys those entries, which shows up as a lockfile diff. Commit it. `pnpm install --frozen-lockfile` installs the lockfile as recorded.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.5
  - @pnpm/deps.path@1101.0.8
  - @pnpm/installing.context@1101.0.13
  - @pnpm/installing.deps-resolver@1102.2.10
  - @pnpm/lockfile.types@1100.1.6
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/pkg-manifest.reader@1100.0.24
  - @pnpm/resolving.git-resolver@1100.1.29
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
