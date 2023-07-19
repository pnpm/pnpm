---
"pnpm": patch
"@pnpm/git-resolver": patch
---

Pass the right scheme to `git ls-remote` in order to prevent a fallback to `git+ssh` that would result in a 'host key verification failed' issue [#6806](https://github.com/pnpm/pnpm/issues/6806)
