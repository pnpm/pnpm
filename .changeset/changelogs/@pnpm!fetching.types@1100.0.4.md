## 1100.0.4

### Patch Changes

- Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.
