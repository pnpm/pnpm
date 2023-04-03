---
"@pnpm/dependency-path": patch
"@pnpm/lockfile-file": patch
"pnpm": patch
---

Repeat installation should work on a project that has a dependency with () chars in the scope name [#6348](https://github.com/pnpm/pnpm/issues/6348).
