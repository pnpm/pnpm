---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

`globalDir` and `globalBinDir` are honored wherever they are set, so `pnpm add -g` no longer fails with `ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH` after `pnpm config set -g global-bin-dir` [#14336](https://github.com/pnpm/pnpm/issues/14336). The global `config.yaml` is read again, `PNPM_CONFIG_GLOBAL_DIR` / `PNPM_CONFIG_GLOBAL_BIN_DIR` reach the directories derived from them, and a leading `~/` is expanded before that derivation. A project's `pnpm-workspace.yaml` still cannot set either key.
