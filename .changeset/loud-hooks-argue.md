---
"pacquet": patch
---

`pnpm install` fails when a `readPackage` hook returns something that is not a package manifest, such as a string. The failure is reported with `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT`. Previously, such a return value was installed as a package with no dependencies at all [#15730](https://github.com/pnpm/pnpm/issues/15730).
