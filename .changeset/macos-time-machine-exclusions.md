---
"pacquet": minor
---

On macOS, `pnpm install` can now exclude newly created modules, virtual-store, and package-store
directories from Time Machine. Set `macosBackup.modulesDir` or `macosBackup.storeDir` to `false`
in global YAML configuration. Environment variables `PNPM_CONFIG_MACOS_BACKUP_MODULES_DIR` and
`PNPM_CONFIG_MACOS_BACKUP_STORE_DIR` are also supported. Project configuration cannot change
these machine-local settings [pnpm/pnpm#6440](https://github.com/pnpm/pnpm/issues/6440).
