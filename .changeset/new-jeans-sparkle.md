---
"@pnpm/lockfile-file": patch
"@pnpm/git-resolver": patch
"pnpm": patch
---

Lockfiles that have git-hosted dependencies specified should be correctly converted to the new lockfile format [#7990](https://github.com/pnpm/pnpm/issues/7990).
