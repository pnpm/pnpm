---
"pacquet": patch
---

Fixed `pnpm install --frozen-lockfile` rejecting a fresh lockfile when an injected workspace dependency has an optional peer supplied by another workspace project [pnpm/pnpm#16428](https://github.com/pnpm/pnpm/issues/16428).
