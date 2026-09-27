---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --offline` now resolves a range to the newest version whose tarball is already in the store. Previously it resolved to the newest version in the cached metadata and failed with `ERR_PNPM_NO_OFFLINE_TARBALL` when that version had not been downloaded [pnpm/pnpm#10715](https://github.com/pnpm/pnpm/issues/10715).
