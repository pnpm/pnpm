---
"pacquet": patch
---

Fixed `pnpm fetch`, `why`, `list`, `ll`, `licenses`, `audit`, `patch`, `sbom`, `peers`, and `runtime` not loading the pnpmfile, so an `updateConfig` hook never applied to them [#14444](https://github.com/pnpm/pnpm/issues/14444). `fetch` failed outright with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when the hook supplied the `catalogs` entry a `catalog:` specifier resolves through; the rest silently discarded whatever the hook changed.
