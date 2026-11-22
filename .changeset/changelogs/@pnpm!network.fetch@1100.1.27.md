## 1100.1.27

### Patch Changes

- Update dependencies.

- Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.6
  - @pnpm/error@1100.2.3
  - @pnpm/fetching.types@1100.0.4
  - @pnpm/types@1102.1.5
