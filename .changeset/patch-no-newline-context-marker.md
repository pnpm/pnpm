---
"pacquet": patch
---

`pnpm install` now applies patches produced by `pnpm patch-commit` when an edit removes the trailing lines of a file along with its newline. The install no longer fails with `ERR_PNPM_INVALID_PATCH` ("expected end of hunk") [#12451](https://github.com/pnpm/pnpm/issues/12451).
