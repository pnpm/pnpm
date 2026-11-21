---
"pacquet": patch
---

`pnpm patch-commit` keeps the peers a patched package is bound to in the lockfile. It no longer fails with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` when the package has an optional peer and `lockfile.format` is `v13`.
