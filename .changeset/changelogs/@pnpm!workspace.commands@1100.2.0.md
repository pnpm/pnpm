## 1100.2.0

### Minor Changes

- `pnpm init` now pins the latest pnpm version, instead of the version of pnpm that ran the command. A project scaffolded by an outdated pnpm therefore no longer inherits that staleness through its own `devEngines.packageManager` / `packageManager` pin [#7490](https://github.com/pnpm/pnpm/issues/7490).

  The version is read from the `latest` tag on the package-manager registries. When that lookup cannot answer — no network, an unreachable or slow registry, `offline`, or a `latest` that the `minimumReleaseAge` / `trustPolicy` settings reject — `pnpm init` pins the running version as before, and never fails or hangs on the lookup. A `latest` that is older than the running pnpm is never pinned either.

### Patch Changes

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.0
  - @pnpm/cli.utils@1101.0.25
  - @pnpm/config.reader@1102.1.0
  - @pnpm/engine.pm.commands@1102.1.0
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.project-manifest-writer@1100.0.16
