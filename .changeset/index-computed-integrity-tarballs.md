---
"@pnpm/worker": patch
"@pnpm/fetching.tarball-fetcher": patch
"pnpm": patch
---

A tarball whose integrity pnpm computed during download is now found in the store on the next install. Before, that install downloaded the tarball again once the lockfile recorded the integrity [#12562](https://github.com/pnpm/pnpm/issues/12562).
