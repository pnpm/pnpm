---
"pnpm": patch
---

Fixed `verifyDepsBeforeRun` incorrectly reporting stale dependencies after an install pruned unused entries from `minimumReleaseAgeExclude` or `trustPolicyExclude` [#18498](https://github.com/pnpm/pnpm/issues/18498).
