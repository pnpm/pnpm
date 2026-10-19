---
"@pnpm/pnpr": minor
---

All configuration sections now strictly validate schema fields and reject unrecognized keys. Configuration keys now consistently accept both camelCase and snake_case names. The retired top-level `uplinks` and `upstreams` blocks are no longer accepted; configure upstreams in the `registries` block instead.
