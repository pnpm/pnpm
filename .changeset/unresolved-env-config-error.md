---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

Throw `ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR` when a configuration setting references an undefined environment variable without a fallback [`pnpm/pnpm#10963`](https://github.com/pnpm/pnpm/pull/10963).
