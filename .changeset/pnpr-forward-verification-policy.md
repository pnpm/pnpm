---
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr.client": patch
"pnpm": patch
---

Installs through a pnpr server now send the whole verification policy to the server, not just `minimumReleaseAge`. Previously `minimumReleaseAgeExclude`, `minimumReleaseAgeIgnoreMissingTime`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, and `trustLockfile` were dropped from the request, so the server enforced a stricter policy than configured — packages listed in `minimumReleaseAgeExclude` were still held back, and a lockfile containing them could be rejected outright.

`trustPolicy: no-downgrade` no longer fails with `TRUST_POLICY_INCOMPATIBLE_WITH_PNPR` when a pnpr server is configured; the server enforces the policy for both reused and freshly-resolved entries.
