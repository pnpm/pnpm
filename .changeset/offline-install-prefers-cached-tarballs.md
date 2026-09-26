---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --offline` now resolves a dependency range to the newest version whose tarball is already in the store. It used to resolve to the newest version in the cached metadata and fail with ERR_PNPM_NO_OFFLINE_TARBALL when that version's tarball was missing [#10715](https://github.com/pnpm/pnpm/issues/10715).
