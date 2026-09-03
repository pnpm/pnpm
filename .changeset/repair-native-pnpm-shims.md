---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

pnpm 11 now runs projects pinned to pnpm 12.3 or later without passing the native pnpm binary to Node.js. The global virtual store now publishes the native pnpm executable directly for these versions [#14502](https://github.com/pnpm/pnpm/issues/14502).
