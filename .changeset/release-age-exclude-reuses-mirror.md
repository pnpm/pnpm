---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

With `minimumReleaseAge` set, packages listed in `minimumReleaseAgeExclude` now resolve from cached metadata without a registry request when the lockfile already has the version their range picks. Packages resolve the same way without `minimumReleaseAge`.
