---
"@pnpm/headless": patch
"pnpm": patch
---

If an external tool or a user have removed a package from node_modules, pnpm should add it back on install. This was only an issue with `node-linker=hoisted`.
