---
"pacquet": patch
---

pnpm now expands a leading `~/` in `storeDir` values loaded from the global config or `PNPM_CONFIG_STORE_DIR` to the user's home directory [pnpm/pnpm#6560](https://github.com/pnpm/pnpm/issues/6560).
