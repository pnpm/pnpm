---
"@pnpm/crypto.polyfill": minor
---

Fix the type of `hash`. It was `any` because `crypto.hash` not being declared would fall back to `any`.
