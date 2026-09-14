---
"pacquet": patch
---

`pnpm outdated --long` fills the Details column with the package homepage again. The command now requests the full package metadata, the only form that carries the homepage field [#14886](https://github.com/pnpm/pnpm/issues/14886).
