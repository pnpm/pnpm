---
"pacquet": patch
---

`pnpm install` now rejects a `readPackage` hook that returns an invalid manifest object with `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT`. Previously, an invalid return value such as a string was treated as a package with no dependencies [pnpm/pnpm#15730](https://github.com/pnpm/pnpm/issues/15730).
