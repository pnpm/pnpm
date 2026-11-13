---
"@pnpm/store.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm cat-file` now reports `ERR_PNPM_INVALID_HASH` when given an empty, malformed, or truncated digest such as `sha512-`.
