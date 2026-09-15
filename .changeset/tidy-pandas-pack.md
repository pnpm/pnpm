---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

Fixed standalone installations to preserve the bundled `node-gyp` files used to build native dependencies.
