---
"@pnpm/global.commands": patch
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update --global` now skips a global package installed from a `file:` path that no longer exists, prints a warning, and updates the remaining global packages. Previously the whole update failed with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` [#12533](https://github.com/pnpm/pnpm/issues/12533).
