---
"@pnpm/pnpr": patch
---

Pipeline run records are now stored with the hosted packages. A run submitted through one replica is listed and served by every other. On a local storage root the records move from `pipeline-runs/v0` to `.pipeline-runs/v0`. Move an existing directory there to keep its runs [#12199](https://github.com/pnpm/pnpm/issues/12199).
