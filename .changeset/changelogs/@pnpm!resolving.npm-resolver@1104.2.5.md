## 1104.2.5

### Patch Changes

- Dependency resolution reads cached registry metadata faster. The metadata cache moved to `<cache-dir>/v12/`, so the first install after upgrading downloads registry metadata again. A damaged cache entry is downloaded again, or reported as an error when `--offline` is set [#13512](https://github.com/pnpm/pnpm/pull/13512).

- Lockfile verification now checks the tarballs inside a `variations` resolution against the registry. A `name@version` lockfile entry with an empty `variations` resolution is now rejected.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.3
  - @pnpm/config.version-policy@1100.2.7
  - @pnpm/constants@1102.0.1
  - @pnpm/crypto.hash@1100.0.8
  - @pnpm/deps.path@1101.0.6
  - @pnpm/error@1100.2.2
  - @pnpm/fs.graceful-fs@1100.2.5
  - @pnpm/pkg-manifest.utils@1100.4.9
  - @pnpm/resolving.jsr-specifier-parser@1100.0.10
  - @pnpm/resolving.registry.pkg-metadata-filter@1100.0.23
  - @pnpm/resolving.tarball-url@1101.1.4
  - @pnpm/store.cafs@1100.3.7
  - @pnpm/store.index@1101.0.1
