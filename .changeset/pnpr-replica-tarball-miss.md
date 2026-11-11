---
"@pnpm/pnpr": patch
---

pnpr instances with an embedded libSQL `replicaPath` now read the primary database when a tarball, archive, or exact version lookup misses the local replica. Fetching a newly published tarball from another instance within `syncInterval` used to fail with HTTP 404.
