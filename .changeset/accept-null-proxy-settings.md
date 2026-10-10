---
"@pnpm/config.reader": patch
"pnpm": patch
---

Treat null or empty `httpProxy` and `httpsProxy` workspace settings as unset rather than raising a type error.
