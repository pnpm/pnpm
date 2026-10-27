---
"pacquet": patch
---

`pnpm --env-file` now expands `$VAR` when the variable name contains `_`, such as `$NPM_TOKEN`. Before, `$T_BASE` expanded as `$T` followed by `_BASE`.

`pnpm --env-file` now loads Windows paths such as `C:\Users\me`, unquoted values with spaces, and values such as `"cost $5"` where a `$` starts no variable name. Before, each of these failed the whole file. Quotes inside an unquoted value, such as `{"k":"v"}`, are now kept.
