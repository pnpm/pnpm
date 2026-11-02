---
"pacquet": patch
---

Concurrent installs that share a global virtual store no longer fail with `ERR_PNPM_CMD_SHIM_SYMLINK_BIN` when they link the same executable at the same time [#17587](https://github.com/pnpm/pnpm/issues/17587).
