---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

A fresh install reusing a warm global virtual store skips reimporting packages whose target directory is already complete [#11112](https://github.com/pnpm/pnpm/issues/11112).
