---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Improve error message when a package version exists but does not meet the `minimumReleaseAge` constraint. The error now clearly states that the version exists and shows a human-readable time since release (e.g., "released 6 hours ago") [#10307](https://github.com/pnpm/pnpm/issues/10307).

