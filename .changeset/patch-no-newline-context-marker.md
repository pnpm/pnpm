---
"pacquet": patch
---

pnpm now applies the patch that `pnpm patch-commit` writes when an edit removes the last lines of a file together with its final newline. Such a patch used to fail with `ERR_PNPM_INVALID_PATCH` and "expected end of hunk" [#12451](https://github.com/pnpm/pnpm/issues/12451).
