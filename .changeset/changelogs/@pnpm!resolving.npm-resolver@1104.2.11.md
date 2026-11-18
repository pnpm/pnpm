## 1104.2.11

### Patch Changes

- Fixed an offline install failing the `minimumReleaseAge` lockfile check with `ERR_PNPM_NO_OFFLINE_META` after an online install resolved the same packages with the same cache directory. The check now reads publish times from the metadata that the resolution cached [#18063](https://github.com/pnpm/pnpm/issues/18063).

- `pnpm install` now re-verifies lockfile resolutions when the default registry or a scoped registry changes [#18070](https://github.com/pnpm/pnpm/pull/18070).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.6
  - @pnpm/config.pick-registry-for-package@1101.0.6
  - @pnpm/config.version-policy@1100.2.11
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/deps.path@1101.0.10
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.26
  - @pnpm/resolving.registry.types@1100.2.6
  - @pnpm/resolving.resolver-base@1101.3.6
  - @pnpm/resolving.tarball-url@1101.1.7
  - @pnpm/store.cafs@1100.3.11
  - @pnpm/types@1102.1.4
