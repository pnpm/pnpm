---
"pnpm": patch
"@pnpm/npm-resolver": patch
"@pnpm/default-reporter": patch
---

When a version specifier cannot be resolved because the versions don't satisfy the `minimumReleaseAge` setting, print this information out in the error message [#9974](https://github.com/pnpm/pnpm/pull/9974).
