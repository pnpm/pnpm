---
"@pnpm/git-resolver": patch
---

Fixed a bug in which pnpm passed the wrong scheme to `git ls-remote`, causing a fallback to `git+ssh` and resulting in a 'host key verification failed' issue [#6805](https://github.com/pnpm/pnpm/issues/6805)
