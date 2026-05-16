---
"@pnpm/resolving.npm-resolver": minor
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Reuse the on-disk full-metadata cache during the `minimumReleaseAge` lockfile revalidation gate. The gate now issues conditional GETs against pnpm's existing metadata mirror — a 304 Not Modified response serves the body from disk instead of refetching the full registry document, so steady-state installs only pay a headers-only round-trip per locked package instead of downloading the full manifest every time [#11675](https://github.com/pnpm/pnpm/issues/11675).
