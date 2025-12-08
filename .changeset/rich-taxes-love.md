---
"pnpm": patch
"@pnpm/npm-resolver": patch
"@pnpm/default-reporter": patch
---

When a version specifier cannot be resolved because the versions don't satisfy the `minimumReleaseAge` setting. Add the latest available dependency versions that match the corresponding configuration to the printed error message.
