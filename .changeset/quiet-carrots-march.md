---
"@pnpm/npm-resolver": patch
"@pnpm/core": patch
"@pnpm/crypto.polyfill": patch
"@pnpm/list": patch
"@pnpm/worker": patch
"pnpm": patch
---

Use `crypto.hash`, when available, for improved performance [#8629](https://github.com/pnpm/pnpm/pull/8629).
