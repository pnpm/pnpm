---
"pnpm": patch
---

On Unix, scripts started through Corepack now receive signals sent to the pnpm PID. This requires Node.js 22.15 or later on the 22.x line, or Node.js 23.11 or later [#18408](https://github.com/pnpm/pnpm/issues/18408).
