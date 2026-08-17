## 1100.1.0

### Minor Changes

- Added `pnpm cache path`, which prints the directory pnpm uses for its metadata cache. CI setups can use it to cache that directory — including the lockfile verification log, which lets a job skip re-checking an unchanged lockfile against the configured supply-chain policies.

### Patch Changes

- Updated dependencies:
  - @pnpm/cache.api@1100.0.38
  - @pnpm/cli.utils@1101.0.23
  - @pnpm/config.reader@1101.17.0
  - @pnpm/error@1100.1.2
  - @pnpm/store.path@1100.0.5
