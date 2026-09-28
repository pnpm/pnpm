---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --offline` and `pnpm add --offline` now resolve a version range to the newest matching version whose tarball is already in the store. They used to pick the newest version in the cached metadata and fail with `ERR_PNPM_NO_OFFLINE_TARBALL` when its tarball was missing [#10715](https://github.com/pnpm/pnpm/issues/10715).
