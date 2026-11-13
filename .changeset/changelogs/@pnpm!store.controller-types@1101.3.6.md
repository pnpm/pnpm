## 1101.3.6

### Patch Changes

- `pnpm deploy --offline --trust-lockfile` now reuses dependencies pinned in the source lockfile from a populated store when registry metadata is unavailable. This works with per-project lockfiles, `--legacy`, and npm aliases [pnpm/pnpm#17888](https://github.com/pnpm/pnpm/issues/17888).

- Updated dependencies:
  - @pnpm/fetching.fetcher-base@1100.2.15
  - @pnpm/resolving.resolver-base@1101.3.5
