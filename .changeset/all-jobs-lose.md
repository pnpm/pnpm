---
'@pnpm/npm-resolver': patch
---

Skip time field validation for packages excluded by `minimumReleaseAgeExclude` (allows packages that would otherwise throw `ERR_PNPM_MISSING_TIME`).
