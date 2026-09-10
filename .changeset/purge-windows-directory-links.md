---
"pacquet": patch
---

On Windows, `pnpm install` no longer fails with `ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` when it has to clear a `node_modules` directory that contains linked dependencies, such as after changing `nodeLinker` [#14790](https://github.com/pnpm/pnpm/issues/14790).
