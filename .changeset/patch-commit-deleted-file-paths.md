---
"pacquet": patch
---

`pnpm patch-commit` now writes a valid patch when a file is deleted in the edit directory. Previously the next `pnpm install` failed with `ERR_PNPM_INVALID_PATCH` [#14559](https://github.com/pnpm/pnpm/issues/14559).
