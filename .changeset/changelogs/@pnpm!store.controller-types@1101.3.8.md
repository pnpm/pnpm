## 1101.3.8

### Patch Changes

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.17
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/types@1102.1.5
