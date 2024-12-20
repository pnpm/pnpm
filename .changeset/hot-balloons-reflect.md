---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Installation with hoisted node_modules should not fail, when a dependency has itself in its own peer dependencies [#8854](https://github.com/pnpm/pnpm/issues/8854).
