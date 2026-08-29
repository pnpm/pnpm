---
"@pnpm/pnpr": patch
---

A shared build artifact publication that cannot unregister itself no longer stops the registry reclaiming space or refusing further publications. A publication says at intervals that it is still working, and a registration that has gone quiet for an hour is written off, so a publication whose bookkeeping write failed stops holding back the collector that reclaims unreferenced blobs and returns the compatibility scopes a failed publication claimed, and stops counting toward the limit on publications in flight.
