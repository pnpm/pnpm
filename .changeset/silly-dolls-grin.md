---
"@pnpm/git-resolver": patch
---

Fix a regression in which pnpm attempted to fetch tarball from codeload.github.com for even private git repo, causing failures [#6827](https://github.com/pnpm/pnpm/issues/6827)
