---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fixed some edge cases where resolving circular peer dependencies caused a dead lock [#8720](https://github.com/pnpm/pnpm/issues/8720).
