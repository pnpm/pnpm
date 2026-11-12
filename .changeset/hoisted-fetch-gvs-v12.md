---
"pacquet": patch
---

`pnpm fetch` with `nodeLinker` set to `hoisted` no longer populates global virtual store links or modifies linked agent skills. `pnpm install` with `frozenStore` enabled no longer fails with `ERR_PNPM_FROZEN_STORE_NEEDS_BUILD` when `nodeLinker` is `hoisted` [#18037](https://github.com/pnpm/pnpm/issues/18037).
