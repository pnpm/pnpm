---
"@pnpm/pnpr": minor
---

pnpr now supports sharing Cargo compilation caches between CI and developers through sccache. Configure `artifacts.compilerCaches` to grant separate read and publication access. sccache can combine its local disk cache with pnpr's remote cache.
