## 0.1.1

### Patch Changes

- With `nodeLinker.type: loaded`, packages that ship their own `node_modules` directory, such as `npm` with its bundled dependencies, now load from the store. One such package in the install stopped every Node.js process from starting.

- With `nodeLinker.type` set to `loaded`, Node.js now stops with `ERR_PNPM_LOADER_UNSUPPORTED_NODE` when it preloads the store loader on a version the loader cannot serve. The supported versions are `^24.18.0 || >=26.2.0`. On other versions, CommonJS packages imported from ESM failed with `Cannot find module` on their first relative `require()`.
