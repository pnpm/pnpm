## 1100.4.0

### Minor Changes

- Lockfile verification now honors offline mode by using cached registry metadata instead of reaching the registry. When the required metadata is not available locally, verification reports the same `ERR_PNPM_NO_OFFLINE_META` condition used by offline resolution.

### Patch Changes

- Updated dependencies:
  - @pnpm/engine.runtime.bun-resolver@1102.0.15
  - @pnpm/engine.runtime.deno-resolver@1102.0.15
  - @pnpm/engine.runtime.node-resolver@1101.1.22
  - @pnpm/resolving.git-resolver@1100.1.16
  - @pnpm/resolving.npm-resolver@1103.2.0
