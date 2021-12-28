---
"@pnpm/resolve-dependencies": patch
---

Injected subdependencies should be hard linked as well. So if `button` is injected into `card` and `card` is injected into `page`, then both `button` and `card` should be injected into `page`.
