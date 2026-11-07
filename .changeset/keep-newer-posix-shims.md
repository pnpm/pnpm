---
"@pnpm/bins.linker": patch
"pnpm": patch
---

Installing no longer replaces the POSIX bin shims that a newer pnpm wrote in `node_modules/.bin`.
