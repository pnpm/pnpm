---
"pacquet": patch
---

`pnpm add -g` and the other `--global` commands now fail with `ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL` when `--virtual-store-dir` is passed. A custom `--hoist-pattern` fails with `ERR_PNPM_CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL`. Passing `--link-workspace-packages`, `--shared-workspace-lockfile`, or `--config.lockfile-dir` with `--global` also fails, with the matching `ERR_PNPM_CONFIG_CONFLICT_*_WITH_GLOBAL` code.
