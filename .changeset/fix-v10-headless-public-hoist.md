---
"@pnpm/headless": patch
"pnpm": patch
---

Recompute hoisted dependencies when a frozen install removes packages, so public hoist links are handed over to retained versions [#16861](https://github.com/pnpm/pnpm/issues/16861).
