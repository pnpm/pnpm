---
"@pnpm/fs.indexed-pkg-importer": patch
"@pnpm/store.controller-types": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now keeps a deployed symlink that points inside the project even if its target is not among the deployed files, such as a link into `node_modules` [#17288](https://github.com/pnpm/pnpm/issues/17288).
