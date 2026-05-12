---
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

`minimumReleaseAge` is now re-applied against the existing lockfile when `pnpm install` would otherwise skip resolution (the lockfile-up-to-date and optimistic-repeat-install fast paths). Previously, a freshly-published version recorded in `pnpm-lock.yaml` (e.g. by a developer who bypassed the policy locally) would be installed by other consumers and CI without being checked, which defeats the security purpose of the setting. The new gate fetches the full registry manifest for every npm-resolved lockfile entry and aborts the install with `ERR_PNPM_MINIMUM_RELEASE_AGE_LOCKFILE_VIOLATION` listing the offending entries. `minimumReleaseAgeExclude` is honored, and missing manifests / unpublished versions are treated as violations rather than silently bypassed. Mirrors the approach taken in bun#30526 for the same shape of issue. [#10438](https://github.com/pnpm/pnpm/issues/10438).
