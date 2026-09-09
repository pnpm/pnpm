---
"pacquet": patch
---

`pnpm install` now reports the dependencies whose build scripts it skipped because their build output was restored from the side-effects cache. Such an install printed nothing at all for those packages, which read as though they had no build scripts. The report names each package, the stages that did not run, and the `sideEffectsCache` setting [#14717](https://github.com/pnpm/pnpm/issues/14717).
