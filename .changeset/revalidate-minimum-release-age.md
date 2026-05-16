---
"@pnpm/installing.deps-installer": minor
"pnpm": patch
---

`minimumReleaseAge` is now re-checked against `pnpm-lock.yaml` before any tarball is installed, so a freshly-published version pinned in the lockfile (e.g. by a developer who bypassed the policy locally) is no longer installed silently by other consumers or CI. Violating entries abort the install with `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION`; `minimumReleaseAgeExclude` is honored. [#10438](https://github.com/pnpm/pnpm/issues/10438).
