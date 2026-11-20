---
"pacquet": patch
---

`pnpm publish` now reads `access` and `tag` from `pnpm-workspace.yaml`, the global `config.yaml`, and `PNPM_CONFIG_ACCESS` / `PNPM_CONFIG_TAG`. The `--access` and `--tag` flags still win when set. A configured `access` outranks `publishConfig.access`, as it does in pnpm. An empty `publishBranch` now allows `master` and `main` [#13542](https://github.com/pnpm/pnpm/issues/13542).
