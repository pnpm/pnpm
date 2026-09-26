---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

A repeat install now keeps the fast path when a declared local file dependency is replaced by an override [pnpm/pnpm#12892](https://github.com/pnpm/pnpm/issues/12892).
