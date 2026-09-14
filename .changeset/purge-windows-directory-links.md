---
"pacquet": patch
---

On Windows, `pnpm install` no longer fails with `ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` when it clears a `node_modules` directory that holds linked dependencies. Changing `nodeLinker` in an installed project hit this [#14790](https://github.com/pnpm/pnpm/issues/14790).
