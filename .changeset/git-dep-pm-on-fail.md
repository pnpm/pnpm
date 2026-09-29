---
"pacquet": patch
---

Git-hosted dependencies now respect `pmOnFail`. If it is set to anything other than `download`, a git-hosted dependency that pins a pnpm version is prepared by the running pnpm, and pnpm does not download the pinned version [#16376](https://github.com/pnpm/pnpm/issues/16376).
