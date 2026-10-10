---
"@pnpm/pnpr": minor
---

`PUT /-/pnpr/v0/artifacts/blob` stores one blob of a signed artifact ahead of the envelope that names it, streamed from the request body. A blob can be up to 4 GiB. `HEAD` on the same URL tells whether a blob is stored.

`artifacts.quota.ownerGiB` and `artifacts.quota.totalGiB` set how much the artifact store keeps. They default to 1 GiB per owner and 10 GiB in total.
