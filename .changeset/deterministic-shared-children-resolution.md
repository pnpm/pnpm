---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Made shared package child resolution deterministic when the same package is reached through multiple contexts. pnpm now chooses the shallowest occurrence, then importer order, then parent path, instead of letting request timing decide the child context and missing-peer report [pnpm/pnpm#12358](https://github.com/pnpm/pnpm/issues/12358).
