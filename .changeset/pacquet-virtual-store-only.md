---
"pacquet": minor
---

Added the `virtualStoreOnly` setting, which populates the virtual store without any post-import linking — no importer symlinks, no `.bin` entries, no hoisting, and no project lifecycle scripts. Combining it with `enableModulesDir: false` fails with `ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR` unless `enableGlobalVirtualStore` is on, since the standard virtual store lives inside `node_modules`. A subsequent ordinary install completes the linking instead of treating the partially-populated directory as up-to-date. `enableModulesDir` is now read from `pnpm-workspace.yaml` as well.
