## 1102.1.0

### Minor Changes

- Added support for registry replacement tarballs using standard integrity values, explicit revision fields, registry routing from the `registries` setting, non-redirecting integrity-addressed URLs, canonical safe-integer revision numbers, and pnpr proxying for immutable upstream revision artifacts.

### Patch Changes

- Enforce `allowBuilds` when a prepared git dependency is reused from the shared store, and use the lockfile's canonical git resolution ID in approval suggestions.

- Updated dependencies:
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/exec.prepare-package@1100.0.34
  - @pnpm/fetching.fetcher-base@1100.2.9
  - @pnpm/fs.graceful-fs@1100.2.0
  - @pnpm/store.index@1100.3.0
  - @pnpm/types@1102.1.0
