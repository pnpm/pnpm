---
"pacquet": patch
---

`pnpm install` now fetches fresh metadata for a package when a version allowed by `minimumReleaseAgeExclude` is missing from the cached metadata. Previously it could fail with `ERR_PNPM_NO_MATCHING_VERSION` without contacting the registry [#17723](https://github.com/pnpm/pnpm/issues/17723).
