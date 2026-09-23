---
"pacquet": minor
---

The global `node` shim created by pnpm now uses the Node.js version from the nearest `.nvmrc` file when the project does not declare a Node.js runtime in `devEngines.runtime` or `engines.runtime` [pnpm/pnpm#4471](https://github.com/pnpm/pnpm/issues/4471). An `.nvmrc` value that only nvm can act on, such as `system` or a custom alias, leaves the global `node` in use.
