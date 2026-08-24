---
"pacquet": patch
"pnpm": patch
---

Don't fail `--frozen-lockfile` when the pinned pnpm version recorded in `pnpm-lock.yaml` has to be re-resolved only because an earlier pnpm wrote its resolutions with tarball URLs. Those entries already record the version the manifest pins, so they are now re-resolved in memory and the lockfile is left untouched, instead of failing with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` [#14124](https://github.com/pnpm/pnpm/issues/14124).
