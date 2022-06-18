---
"@pnpm/core": patch
"@pnpm/headless": patch
"pnpm": patch
---

Packages that should be built are always cloned or copied from the store. This is required to prevent the postinstall scripts from modifying the original source files of the package.
