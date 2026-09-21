---
"@pnpm/config.reader": patch
"@pnpm/store.connection-manager": patch
"pnpm": patch
---

`CreateNewStoreControllerOptions` is now exported from `@pnpm/store.connection-manager`. Store controllers now read custom certificate authorities from `cafile`.
