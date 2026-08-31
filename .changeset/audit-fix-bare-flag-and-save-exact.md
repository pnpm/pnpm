---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm audit --fix` failing with `ERR_PNPM_INVALID_FIX_OPTION` when used without a value, including when another flag follows it, as in `pnpm audit --fix --json` [#13261](https://github.com/pnpm/pnpm/issues/13261). Fixed `pnpm audit --fix=override` ignoring the `saveExact` and `savePrefix` settings when writing vulnerability overrides [#11523](https://github.com/pnpm/pnpm/issues/11523).
