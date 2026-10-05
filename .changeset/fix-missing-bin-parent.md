---
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_CMD_SHIM_RESOLVE_PATH` when an executable's parent directory contains a dangling symlink.
