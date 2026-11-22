## 1102.2.12

### Patch Changes

- `pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.8
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/exec.prepare-package@1100.0.50
  - @pnpm/fetching.fetcher-base@1100.2.17
  - @pnpm/fetching.pick-fetcher@1100.1.19
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/hooks.types@1101.0.10
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/store.cafs@1100.3.12
  - @pnpm/store.controller-types@1101.3.8
  - @pnpm/store.index@1101.0.5
  - @pnpm/types@1102.1.5
