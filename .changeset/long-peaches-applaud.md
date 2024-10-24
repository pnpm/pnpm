---
"pnpm": patch
---

Don't validate (and possibly purge) `node_modules` in commands which should not modify it (e.g. `pnpm install --lockfile-only`) [#8657](https://github.com/pnpm/pnpm/pull/8657).
