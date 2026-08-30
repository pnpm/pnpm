## 1102.1.0

### Minor Changes

- Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.

### Patch Changes

- Fixed `ERR_PNPM_UNUSED_PATCH` validation during incremental installs [pnpm/pnpm#13692](https://github.com/pnpm/pnpm/issues/13692).

- `pnpm update` no longer replaces the specifier a project declares for a dependency that is also listed in `overrides`. A `catalog:` reference stays a `catalog:` reference, and a declared range stays as written, instead of being rewritten to the version the override resolved to [#12115](https://github.com/pnpm/pnpm/issues/12115).

- `pnpm update` no longer moves the range a project declares for a dependency that `overrides` also lists, even when the override repeats that range verbatim. Previously the updated `package.json` disagreed with the lockfile, so the next `pnpm install --frozen-lockfile` failed with a specifier mismatch [#14224](https://github.com/pnpm/pnpm/issues/14224).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.1
  - @pnpm/config.version-policy@1100.2.2
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/deps.graph-hasher@1100.3.0
  - @pnpm/deps.path@1101.0.1
  - @pnpm/fetching.pick-fetcher@1100.1.9
  - @pnpm/fs.symlink-dependency@1100.0.19
  - @pnpm/hooks.types@1101.0.1
  - @pnpm/lockfile.preferred-versions@1100.0.31
  - @pnpm/lockfile.pruner@1100.0.21
  - @pnpm/lockfile.types@1100.1.0
  - @pnpm/lockfile.utils@1102.1.0
  - @pnpm/patching.config@1100.1.3
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/pkg-manifest.utils@1100.4.2
  - @pnpm/resolving.npm-resolver@1104.1.0
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/store.controller-types@1101.2.0
  - @pnpm/types@1102.1.0
