---
"@pnpm/config.reader": patch
"pnpm": patch
---

A setting in `pnpm-workspace.yaml` or the global `config.yaml` that references an undefined environment variable without a fallback now fails with `ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR`. pnpm used to fail with an error that had no code [#10963](https://github.com/pnpm/pnpm/pull/10963).
