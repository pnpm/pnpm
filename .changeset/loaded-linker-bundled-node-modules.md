---
"@pnpm/esm-loader": patch
"pacquet": patch
---

With `nodeLinker.type: loaded`, packages that ship their own `node_modules` directory, such as `npm` with its bundled dependencies, now load from the store. One such package in the install stopped every Node.js process from starting.
