---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`trustPolicy: no-downgrade` no longer aborts the install with `ERR_PNPM_MISSING_TIME` on registries that serve no per-version `time` field when `minimumReleaseAgeIgnoreMissingTime` is set. The trust check reads the same publish dates the `minimumReleaseAge` check does, so it now honors the same opt-in and skips the affected package with a warning [#12446](https://github.com/pnpm/pnpm/issues/12446).

`minimumReleaseAgeIgnoreMissingTime` no longer lets a lockfile entry the registry does not list pass the `minimumReleaseAge` check during lockfile verification. The opt-in covers a registry that cannot date its releases; a packument that does date every version it lists is saying it never published this one, which stays a hard failure.

The missing-`time` warning now names the check it is reporting on, so a package whose `minimumReleaseAge` and `trustPolicy` checks are both skipped warns about both instead of only the first.
