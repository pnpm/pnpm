---
"pacquet": patch
---

`pnpm patch-commit` now writes a valid `diff --git a/<file> b/<file>` header for a file deleted in the edit directory. The `b/` prefix lost its slash, so the next `pnpm install` failed with `ERR_PNPM_INVALID_PATCH` [#14559](https://github.com/pnpm/pnpm/issues/14559).
