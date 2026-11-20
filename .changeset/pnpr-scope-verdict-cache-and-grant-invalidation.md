---
"@pnpm/pnpr": patch
---

`POST /-/pnpr/v0/verify-lockfile` and `POST /-/pnpr/v0/resolve` now enforce caller read permissions for every private route recorded in a cached lockfile verdict. Updating grants through the admin API now invalidates cached resolutions and lockfile verdicts immediately. A running server applies a `pnpr grants import` within about 10 seconds.
