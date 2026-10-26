---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit --fix` and `pnpm audit --interactive` now fail with `ERR_PNPM_AUDIT_FIX_WITH_IGNORE` when combined with `--ignore` or `--ignore-unfixable`. Before, the fix ran and the ignore flags had no effect.
