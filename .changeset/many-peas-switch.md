---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fix `Cannot read properties of undefined (reading 'name')` that is printed while trying to render the missing peer dependencies warning message [#8538](https://github.com/pnpm/pnpm/issues/8538).
