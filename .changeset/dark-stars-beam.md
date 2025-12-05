---
"@pnpm/npm-resolver": patch
---

Fixed time field check in trust policy for packages excluded from trust policy checks.
This fixes the ERR_PNPM_MISSING_TIME error that occurs when the trust policy is set to
`no-downgrade` and the package is missing a time field despite being excluded.
