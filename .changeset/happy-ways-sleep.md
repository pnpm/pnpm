---
"@pnpm/store.index": minor
---

Export `pickStoreIndexKey(resolution, pkgId, { built })` — picks the appropriate store-index key for a resolution:
git-hosted entries route through `gitHostedStoreIndexKey(pkgId, { built })`, everything else through
`storeIndexKey(resolution.integrity, pkgId)`. Centralizes the routing for `installing.package-requester`,
`building.after-install`, `store.pkg-finder`, and `modules-mounter.daemon` so each consumer reads
`resolution.gitHosted` once via a single typed call.

