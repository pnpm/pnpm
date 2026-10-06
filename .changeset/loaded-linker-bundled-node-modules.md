---
"@pnpm/esm-loader": patch
"pacquet": patch
---

With `nodeLinker.type: loaded`, a package that ships files inside its own `node_modules` directory no longer stops every Node.js process from starting. Packages with bundled dependencies, such as `npm`, now load those dependencies from the store.
