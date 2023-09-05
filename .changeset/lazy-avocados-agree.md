---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Fixes a regression published with pnpm v8.7.3. Don't hang while reading `package.json` from the content-addressable store [#7051](https://github.com/pnpm/pnpm/pull/7051).
