---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` rejecting an up-to-date lockfile when an injected workspace package uses a catalog entry in `peerDependencies` [pnpm/pnpm#16557](https://github.com/pnpm/pnpm/issues/16557).
