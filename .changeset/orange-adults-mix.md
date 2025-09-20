---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

When `minimumReleaseAge` is set and the `latest` tag is not mature enough, prefer a non-deprecated version as the new `latest` [#9987](https://github.com/pnpm/pnpm/issues/9987).
