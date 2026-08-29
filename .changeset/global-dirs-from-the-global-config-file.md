---
"pacquet": patch
---

`globalDir` and `globalBinDir` set in the global `config.yaml` are honored again, so `pnpm add -g` no longer fails with `ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH` after `pnpm config set -g global-bin-dir` [#14336](https://github.com/pnpm/pnpm/issues/14336). Both settings were only read from `PNPM_CONFIG_GLOBAL_DIR` / `PNPM_CONFIG_GLOBAL_BIN_DIR`. A project's `pnpm-workspace.yaml` still cannot set them.
