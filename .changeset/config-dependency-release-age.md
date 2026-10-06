---
"@pnpm/installing.env-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer rejects a locked config dependency for being newer than `minimumReleaseAge` [#16660](https://github.com/pnpm/pnpm/issues/16660).
