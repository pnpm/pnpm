---
"pacquet": patch
---

`pnpm login` / `pnpm adduser` now read the `scope` setting from `pnpm-workspace.yaml`, the global `config.yaml`, and the `PNPM_CONFIG_SCOPE` environment variable, not only from the `--scope` command-line flag. When `scope` is configured, the granted token is keyed to that scope and the scope-to-registry mapping is recorded. `--scope` still takes precedence when both are set. Note that `scope` in an `.npmrc` is not read — pnpm keeps only auth and registry keys from that file.
