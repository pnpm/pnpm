---
"@pnpm/esm-loader": patch
"pacquet": patch
---

With `nodeLinker.type: loaded`, a package that ships files inside its own `node_modules` directory no longer stops every Node.js process from starting. The loader now ignores those files.
