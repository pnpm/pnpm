---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

`.npmrc` files now support npm's `${VAR?}` placeholder. It expands to the value of `VAR`, or to an empty string without a warning when `VAR` is unset [#14404](https://github.com/pnpm/pnpm/issues/14404).
