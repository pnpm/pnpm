---
"pacquet": patch
---

pnpm 11 releases older than 11.28.4 can run pnpm 12 again when the `packageManager` field pins it. Since 12.9.0 they failed with `SyntaxError: Invalid or unexpected token` [#16594](https://github.com/pnpm/pnpm/issues/16594).
