## 1101.0.8

### Patch Changes

- Registry error messages now always say "(response body truncated)" when pnpm cut the response body short. The marker was missing when the body was cut at exactly 64 KiB.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.2
  - @pnpm/network.fetch@1100.1.20
  - @pnpm/registry-access.client@1100.1.22
