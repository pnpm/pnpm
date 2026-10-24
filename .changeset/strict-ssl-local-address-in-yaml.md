---
"pacquet": patch
---

pnpm now reads the `strictSsl` and `localAddress` settings from `pnpm-workspace.yaml`, the global `config.yaml`, `PNPM_CONFIG_*` environment variables, and `--config.*` options. Before, pnpm read them only from `.npmrc` as `strict-ssl` and `local-address` and ignored the other sources.
