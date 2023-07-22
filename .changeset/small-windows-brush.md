---
"@pnpm/prepare-package": patch
"pnpm": patch
---

The "postpublish" script of a git-hosted dependency is not executed, while building the dependency [#6822](https://github.com/pnpm/pnpm/issues/6846).
