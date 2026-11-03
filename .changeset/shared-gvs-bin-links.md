---
"pacquet": patch
---

Concurrent installs that share a global virtual store could fail with `ERR_PNPM_CMD_SHIM_SYMLINK_BIN` when they linked the same executable at the same time. They now accept the link the other install created [#17587](https://github.com/pnpm/pnpm/issues/17587).
