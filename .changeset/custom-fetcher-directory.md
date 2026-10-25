---
"pacquet": patch
---

A custom fetcher in the pnpmfile can now call `fetchers.directory` to fetch a package from a local directory. The call rejects with `ERR_PNPM_TARBALL_INTEGRITY` if the lockfile pins an archive integrity for the package. Previously, calling `fetchers.directory` failed with a `TypeError`.
