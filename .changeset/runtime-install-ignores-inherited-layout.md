---
"pacquet": patch
---

A global command such as `node` now runs the version a project pins when `PNPM_CONFIG_LOCKFILE_DIR`, `PNPM_CONFIG_VIRTUAL_STORE_ONLY`, or `PNPM_CONFIG_SYMLINK` is set in the environment. Before, the dispatch failed with "the installed node executable is not under" the store.
