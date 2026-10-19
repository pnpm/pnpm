---
"pacquet": patch
---

`pnpm patch-commit` and `pnpm install` now apply a patch that renames or moves a file of the patched package. Such a patch failed with "rename/copy operations in patches are not yet supported", including one written by pnpm 11.
