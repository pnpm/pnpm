## 1104.1.0

### Minor Changes

- Added explicit registry revision selection with `<version>+rN` and `pnpm update --patches` for refreshing revision artifacts without changing package versions. Registry-backed lockfile policy checks recognize historical revisions, and pnpr now preserves safe revision histories from upstream registries.

- Added support for registry replacement tarballs using standard integrity values, explicit revision fields, registry routing from the `registries` setting, non-redirecting integrity-addressed URLs, canonical safe-integer revision numbers, and pnpr proxying for immutable upstream revision artifacts.

### Patch Changes

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.1
  - @pnpm/config.pick-registry-for-package@1101.0.1
  - @pnpm/config.version-policy@1100.2.2
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/crypto.hash@1100.0.3
  - @pnpm/deps.path@1101.0.1
  - @pnpm/fs.graceful-fs@1100.2.0
  - @pnpm/pkg-manifest.utils@1100.4.2
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.18
  - @pnpm/resolving.registry.types@1100.2.0
  - @pnpm/resolving.resolver-base@1101.2.0
  - @pnpm/resolving.tarball-url@1101.1.0
  - @pnpm/store.cafs@1100.3.0
  - @pnpm/store.index@1100.3.0
  - @pnpm/types@1102.1.0
