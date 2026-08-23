## 1100.1.8

### Patch Changes

- A custom fetcher can no longer replace the archive integrity that `pnpm-lock.yaml` pins: the locked value is restored after a `canFetch` or `fetch` hook rewrites the resolution, and delegating a locked archive to a directory or git source now fails instead of installing unverified content.

  The Rust CLI now also loads the pnpmfiles named by the `pnpmfile` setting (a single path or an ordered list), and hands custom fetchers native `localTarball` and `remoteTarball` callbacks — including on a fresh install that has to compute a missing tarball integrity, which is then reused by later offline installs. File maps a fetcher returns are accepted only when they match what those native callbacks extracted.

- Updated dependencies:
  - @pnpm/error@1100.1.3
  - @pnpm/fetching.fetcher-base@1100.2.8
  - @pnpm/hooks.types@1101.0.0
  - @pnpm/resolving.resolver-base@1101.1.1
