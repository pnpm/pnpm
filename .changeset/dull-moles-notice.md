---
"@pnpm/core": patch
"@pnpm/cafs": patch
"pnpm": patch
---

Installation of a git-hosted dependency without `package.json` should not fail, when the dependency is read from cache [#6721](https://github.com/pnpm/pnpm/issues/6721).
