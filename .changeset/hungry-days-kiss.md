---
"@pnpm/resolve-dependencies": patch
"@pnpm/lockfile-utils": patch
"pnpm": patch
---

Fix a bug where `--fix-lockfile` crashes on tarballs [#7368](https://github.com/pnpm/pnpm/issues/7368).
