---
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr.client": patch
"pnpm": patch
---

`pnpm install` with `pnprServer` now rejects registry-keyed lockfile entries whose resolution uses a non-registry source with `ERR_PNPM_RESOLUTION_SHAPE_MISMATCH`. The `pnpr` lockfile verification and resolution endpoints enforce the same check before accepting or caching a lockfile.
