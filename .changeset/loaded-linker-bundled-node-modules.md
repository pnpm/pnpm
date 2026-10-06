---
"pacquet": patch
---

With `nodeLinker.type: loaded`, a package that ships files inside its own `node_modules` directory no longer stops every Node.js process from starting. pnpm leaves those files out of `.pnpm-store.json`.
