---
"pacquet": patch
---

`pnpm install` now rejects `readPackage` hooks that return a non-object value. Invalid hook results report `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT` [#15730](https://github.com/pnpm/pnpm/issues/15730).
