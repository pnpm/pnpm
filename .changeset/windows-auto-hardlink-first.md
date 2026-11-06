---
"pacquet": patch
---

`pnpm install` now tries hardlinks before cloning on Windows when `packageImportMethod` is `auto`. This speeds up installs on ReFS Dev Drives [#17809](https://github.com/pnpm/pnpm/issues/17809).
