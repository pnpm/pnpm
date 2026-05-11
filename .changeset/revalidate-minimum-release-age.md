---
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

`minimumReleaseAge` is now re-applied against the existing lockfile when `pnpm install` would otherwise skip resolution. Previously, a freshly-published version recorded in `pnpm-lock.yaml` (e.g. by a developer who bypassed the policy locally) would be installed by other consumers and CI without being checked, which defeats the security purpose of the setting. Installs now fail with `ERR_PNPM_MINIMUM_RELEASE_AGE_LOCKFILE_VIOLATION` listing the offending entries. `minimumReleaseAgeExclude` is respected. [#10438](https://github.com/pnpm/pnpm/issues/10438).
