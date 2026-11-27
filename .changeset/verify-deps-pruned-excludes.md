---
"pnpm": patch
---

Fixed `verifyDepsBeforeRun` requesting a second install after pnpm prunes unused `minimumReleaseAgeExclude` or `trustPolicyExclude` entries [#18498](https://github.com/pnpm/pnpm/issues/18498).
