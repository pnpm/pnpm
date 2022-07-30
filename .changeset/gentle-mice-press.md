---
"@pnpm/core": patch
"pnpm": patch
---

Don't incorrectly consider a lockfile out-of-date when `workspace:^` or `workspace:~` version specs are used in a workspace.
