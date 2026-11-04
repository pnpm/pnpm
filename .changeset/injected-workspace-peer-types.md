---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` rejecting a freshly generated lockfile when `addMissingPeerTypes` is enabled and an injected workspace package has peer dependencies. Fixes [pnpm/pnpm#17668](https://github.com/pnpm/pnpm/issues/17668).
