## 1100.1.22

### Patch Changes

- Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.

- Tarball URL preflight requests use the configured credentials and fetch retry settings.

- Updated dependencies:
  - @pnpm/fetching.types@1100.0.4
  - @pnpm/network.fetch@1100.1.27
  - @pnpm/resolving.resolver-base@1101.3.7
