---
"pacquet": patch
---

`pnpm --env-file` now expands `$VAR` when the variable name contains `_`, such as `$NPM_TOKEN`. Before, `$T_BASE` expanded as `$T` followed by `_BASE`.

`pnpm --env-file` no longer rejects Windows paths such as `C:\Users\me`, a `$` that starts no variable name, such as `cost $5`, or unquoted values with spaces. Quotes inside an unquoted value are kept.
