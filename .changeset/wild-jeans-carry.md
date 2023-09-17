---
"@pnpm/headless": patch
"pnpm": patch
---

Fixed an ENOENT error that was sometimes happening during install with "hoisted" `node_modules` [#6756](https://github.com/pnpm/pnpm/issues/6756).
