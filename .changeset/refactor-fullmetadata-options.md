---
"@pnpm/fetch": major
---

Refactored `fullMetadata` option handling. The `fullMetadata` option is no longer accepted by `createFetchFromRegistry()` at construction time - it should only be passed at call time via the fetch options.
