---
"pacquet": patch
---

`pnpm add yarn@<version>` now records just the resolved version in the `packageManager` field, without corepack's integrity hash. Corepack verifies the release it downloads on its own, so the hash only added a second copy of that information to a field pnpm never verifies.
