---
"@pnpm/store.cafs": patch
"pnpm": patch
---

Fixes a regression published with pnpm v8.7.3. Don't while reading `package.json` from the content-addressable store.
