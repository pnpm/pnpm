## 1101.1.0

### Minor Changes

- `pnpm init` now pins the latest pnpm version, instead of the version of pnpm that ran the command. A project scaffolded by an outdated pnpm therefore no longer inherits that staleness through its own `devEngines.packageManager` / `packageManager` pin [#7490](https://github.com/pnpm/pnpm/issues/7490).

  The version is read from the `latest` tag on the package-manager registries. When that lookup cannot answer — no network, an unreachable or slow registry, `offline`, or a `latest` that the `minimumReleaseAge` / `trustPolicy` settings reject — `pnpm init` pins the running version as before, and never fails or hangs on the lookup. A `latest` that is older than the running pnpm is never pinned either.

### Patch Changes

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.0
  - @pnpm/config.normalize-registries@1101.0.1
  - @pnpm/config.reader@1102.1.0
  - @pnpm/installing.client@1100.3.7
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/store.controller@1102.1.0
  - @pnpm/store.index@1100.3.0
  - @pnpm/types@1102.1.0
