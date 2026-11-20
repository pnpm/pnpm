---
"@pnpm/pnpr": minor
---

pnpr stores hosted packages in a new layout. Each org has its own namespace, and every package keeps its document and blobs in separate places. The layout is the same on disk and in S3.

pnpr no longer starts on a store in the earlier layout. Back the store up and convert it with `pnpr upgrade-storage`. On first start, pnpr also empties the proxy cache, which refills on demand.
