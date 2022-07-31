---
"@pnpm/npm-resolver": minor
"@pnpm/types": minor
"pnpm": minor
---

When `publishConfig.directory` is set, only symlink it to other workspace projects if `publishConfig.linkDirectory` is set to `true`. Otherwise, only use it for publishing [#5115](https://github.com/pnpm/pnpm/issues/5115).
