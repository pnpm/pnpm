---
"pacquet": minor
---

The global `node` shim created by pnpm now reads `.node-version` files as well as `.nvmrc` files. The nearest directory with a Node.js runtime declaration decides the version. Within one directory, `package.json` takes precedence over `.node-version`, which takes precedence over `.nvmrc`.
