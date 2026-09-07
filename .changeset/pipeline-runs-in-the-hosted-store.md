---
"@pnpm/pnpr": patch
---

Pipeline run records are now stored with the hosted packages, so a run submitted through one replica is listed and served by every other. On a local storage root they move from `<storage>/pipeline-runs/v0` to `<storage>/.pipeline-runs/v0`; move an existing directory there to keep its runs [#12199](https://github.com/pnpm/pnpm/issues/12199).
