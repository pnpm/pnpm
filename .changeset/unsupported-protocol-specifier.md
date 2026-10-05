---
"@pnpm/resolving.local-resolver": patch
"pnpm": patch
---

`pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a dependency uses a specifier with a protocol pnpm does not support, such as Yarn's `patch:`. pnpm linked such a dependency to a directory that does not exist [#16590](https://github.com/pnpm/pnpm/issues/16590).
