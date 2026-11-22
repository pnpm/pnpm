---
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/fetching.types": patch
"@pnpm/network.fetch": patch
"@pnpm/resolving.default-resolver": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.tarball-resolver": patch
"pnpm": patch
"pacquet": patch
---

Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.
