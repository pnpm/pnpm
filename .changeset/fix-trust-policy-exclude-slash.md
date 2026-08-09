---
"@pnpm/deps.path": patch
"@pnpm/config.version-policy": patch
"pnpm": patch
"pacquet": patch
---

Fixed an issue where `trustPolicyExclude` failed to exempt a downgraded package in the lockfile if the dependency path key started with a leading slash. Also improved configuration parsing robustness to correctly handle `trustPolicyExclude` and `minimumReleaseAgeExclude` when specified as a single string instead of an array.
