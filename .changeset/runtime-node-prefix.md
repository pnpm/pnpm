---
"@pnpm/engine.runtime.node-resolver": patch
"pnpm": patch
---

Preserve the existing Node.js runtime version prefix when resolving `node@runtime:<range>` to a concrete version.
