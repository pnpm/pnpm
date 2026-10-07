---
"@pnpm/installing.linking.hoist": patch
"pnpm": patch
---

Fixed `pnpm install` failing on Windows with `EPERM` on `rmdir node_modules/@scope` when several workspace packages under one scope are hoisted, for example with `nodeLinker: hoisted` [#16690](https://github.com/pnpm/pnpm/issues/16690).
