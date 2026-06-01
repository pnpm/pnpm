---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Preserve the `integrity` field of a remote (non-registry) tarball dependency when its lockfile entry is rebuilt. Re-resolving such a dependency without re-fetching it (for example via `pnpm update`, or when another dependency changes) produced a resolution with no integrity — URL/tarball resolvers only learn the integrity after the tarball is downloaded — so the previously recorded integrity was dropped, making later installs fail with `ERR_PNPM_MISSING_TARBALL_INTEGRITY` [#12067](https://github.com/pnpm/pnpm/issues/12067).
