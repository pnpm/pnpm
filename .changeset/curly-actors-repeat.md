---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

A dependency is hoisted to resolve an optional peer dependency only if it satisfies the range provided for the optional peer dependency [#8028](https://github.com/pnpm/pnpm/pull/8028).
