## 1102.3.0

### Minor Changes

- When an overrides entry requests a version that does not exist on the registry, `pnpm install` now names that override entry in the error. If the entry's selector has a version range, such as `foo@^1`, the error also shows the latest published version in that range.

### Patch Changes

- Frozen installs now detect changes to catalog entries used only by peer dependencies of local directory packages.

- `pnpm install` and `pnpm install --frozen-lockfile` now enforce `blockExoticSubdeps` against entries in `pnpm-lock.yaml`. Before, pnpm accepted a lockfile with git or tarball URL subdependencies and installed those packages even with `blockExoticSubdeps: true`.

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- Fixed `pnpm install --lockfile-only` fetching unchanged Git dependencies when an unrelated dependency changes and `trustLockfile` is enabled [#18217](https://github.com/pnpm/pnpm/issues/18217).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/config.parse-overrides@1100.1.9
  - @pnpm/config.pick-registry-for-package@1101.0.7
  - @pnpm/config.version-policy@1100.2.12
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/deps.graph-hasher@1100.3.14
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/fetching.pick-fetcher@1100.1.19
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/fs.symlink-dependency@1100.0.28
  - @pnpm/hooks.types@1101.0.10
  - @pnpm/lockfile.preferred-versions@1100.0.45
  - @pnpm/lockfile.pruner@1100.0.32
  - @pnpm/lockfile.types@1100.1.9
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/patching.config@1100.1.14
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/resolving.jsr-specifier-parser@1100.0.11
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/store.controller-types@1101.3.8
  - @pnpm/types@1102.1.5
