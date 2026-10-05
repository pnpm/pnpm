---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

pnpm now fails with `ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR` when a setting in `pnpm-workspace.yaml` or the global `config.yaml` references an undefined environment variable that has no fallback [#10963](https://github.com/pnpm/pnpm/pull/10963).
