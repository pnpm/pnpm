---
"pacquet": patch
---

`ERR_PNPM_PACKAGE_MANAGER_REMOVE_MODULES_DIR` now names the file or directory that could not be removed. It previously reported only the underlying OS error, so a message such as "Access is denied (os error 5)" gave no way to tell which entry under `node_modules` was at fault.
