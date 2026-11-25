---
"pnpm": patch
---

Fixed exact versions with SemVer build metadata in `minimumReleaseAgeExclude`, `trustPolicyExclude`, and `allowBuilds`. Pruning now keeps exclusions for versions in the lockfile [#18444](https://github.com/pnpm/pnpm/issues/18444).
