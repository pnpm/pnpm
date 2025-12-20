---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Don't fail with a `ERR_PNPM_MISSING_TIME` error if a package that is excluded from trust policy checks is missing the time field in the metadata.
