---
"pacquet": patch
---

A `readPackage` hook that sets a dependency range to a value that is not a string now fails `pnpm install` with `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT`. The dependency used to be dropped without a warning [#15705](https://github.com/pnpm/pnpm/issues/15705).
