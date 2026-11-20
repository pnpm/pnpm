---
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_NEXT_LOCKFILE_NOT_REPRESENTABLE` under `lockfile.format: v13` when a peer dependency is provided by an aliased package and another alias installs the package that has the peer's name [#18198](https://github.com/pnpm/pnpm/issues/18198).
