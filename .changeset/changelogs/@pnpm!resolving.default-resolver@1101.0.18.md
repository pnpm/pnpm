## 1101.0.18

### Patch Changes

- Tarball downloads send registry credentials only over HTTPS or loopback HTTP. Tarball and package metadata requests respect registry credential paths on redirects. A failed tarball request can no longer pin its redirect destination in the lockfile.

- Tarball URL preflight requests use the configured credentials and fetch retry settings.

- Updated dependencies:
  - @pnpm/engine.runtime.bun-resolver@1102.0.34
  - @pnpm/engine.runtime.deno-resolver@1102.0.34
  - @pnpm/engine.runtime.node-resolver@1101.3.15
  - @pnpm/error@1100.2.3
  - @pnpm/fetching.types@1100.0.4
  - @pnpm/hooks.types@1101.0.10
  - @pnpm/network.auth-header@1101.1.20
  - @pnpm/resolving.git-resolver@1100.1.32
  - @pnpm/resolving.local-resolver@1101.2.11
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/resolving.resolver-base@1101.3.7
  - @pnpm/resolving.tarball-resolver@1100.1.22
  - @pnpm/types@1102.1.5
