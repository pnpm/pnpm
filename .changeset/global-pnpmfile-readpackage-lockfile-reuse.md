---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Fix an issue where changes to a global `readPackage` hook could be ignored during `pnpm install` [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).
