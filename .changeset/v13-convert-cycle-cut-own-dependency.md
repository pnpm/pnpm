---
"pacquet": patch
---

`pnpm install` with `lockfile.format: v13` no longer fails with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` when it converts a `v9` lockfile in which a package with many peers has a hashed peer suffix and is in a dependency cycle with one of its peers [pnpm/pnpm#18148](https://github.com/pnpm/pnpm/issues/18148).
