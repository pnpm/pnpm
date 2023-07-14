---
"@pnpm/git-resolver": patch
---

Fix a bug where pnpm pass the wrong scheme to `git ls-remote`, causing it to fallback to `git+ssh`, creating the host key verification failed issue [#6805](https://github.com/pnpm/pnpm/issues/6805).
