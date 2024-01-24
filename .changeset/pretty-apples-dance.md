---
"@pnpm/fs.indexed-pkg-importer": patch
"pnpm": patch
---

Prefer hard links over reflinks on Windows as they perform better [#7564](https://github.com/pnpm/pnpm/pull/7564).
