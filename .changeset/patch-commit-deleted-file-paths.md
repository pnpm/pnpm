---
"pacquet": patch
---

`pnpm patch-commit` now writes a valid patch when a file is added to or deleted from the edit directory. `pnpm install` now applies a patch that deletes a file without listing its contents, including patches written by pnpm 11. Deleting a file made the next install fail with `ERR_PNPM_INVALID_PATCH` [#14559](https://github.com/pnpm/pnpm/issues/14559).
