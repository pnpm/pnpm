---
"@pnpm/deps.graph-builder": patch
"@pnpm/store.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm store status` reporting untouched packages as modified when the global virtual store is enabled. If an installed global-store package cannot be located, the command reports `ERR_PNPM_STORE_STATUS_UNVERIFIABLE` and asks for a reinstall. [pnpm/pnpm#17631](https://github.com/pnpm/pnpm/issues/17631).
