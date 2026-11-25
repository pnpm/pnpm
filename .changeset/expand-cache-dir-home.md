---
"pnpm": patch
---

pnpm now expands a leading `~/` in `cacheDir` values loaded from the global config or `PNPM_CONFIG_CACHE_DIR` to the user's home directory, as it does for `storeDir`.
