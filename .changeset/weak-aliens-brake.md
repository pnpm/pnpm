---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

When `minimumReleaseAge` is set and the active version under a dist-tag is not mature enough, do not downgrade to a prerelease version in case the original version wasn't a prerelease one [#9979](https://github.com/pnpm/pnpm/issues/9979).
