---
"@pnpm/patching.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm patch-commit` no longer fails with `ERR_PNPM_INVALID_PATCH` and a `Bad diff line` error when the project directory or the edit directory contains non-ASCII characters [#13801](https://github.com/pnpm/pnpm/issues/13801).
