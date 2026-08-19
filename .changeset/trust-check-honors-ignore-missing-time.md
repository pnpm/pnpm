---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`trustPolicy: no-downgrade` no longer aborts the install with `ERR_PNPM_MISSING_TIME` on registries that serve no per-version `time` field when `minimumReleaseAgeIgnoreMissingTime` is set. The trust check reads the same publish dates the `minimumReleaseAge` check does, so it now honors the same opt-in and skips the affected package with a warning [#12446](https://github.com/pnpm/pnpm/issues/12446). A packument that dates its other versions but not the one being installed still fails, since that is a gap in the metadata rather than a registry that omits the field.

The missing-`time` warning now names each check it is reporting on, so a package whose `minimumReleaseAge` and `trustPolicy` checks are both skipped warns about both instead of only the first.
