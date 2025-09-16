---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

Don't ignore the `minimumReleaseAge` check, when the package is requested by exact version and the packument is loaded from cache [#9978](https://github.com/pnpm/pnpm/issues/9978).
