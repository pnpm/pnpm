---
"@pnpm/pnpr": patch
---

The pnpr config file now supports `${VAR?}` placeholders. They expand to the value of `VAR`, or to an empty string without a warning when `VAR` is unset [#14404](https://github.com/pnpm/pnpm/issues/14404).
