---
"pacquet": patch
---

`ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` now names the file or directory in `node_modules` that pnpm could not clean up. It previously reported only the underlying OS error, such as "Access is denied (os error 5)".
