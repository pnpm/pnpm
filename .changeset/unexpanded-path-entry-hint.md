---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

When the global bin directory is not in `PATH` and a `PATH` entry still contains an unexpanded environment variable reference such as `%PNPM_HOME%`, the `ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH` error now names that entry and explains that, on Windows, the referenced user variable must be set and stored as a plain string (`REG_SZ`), not an expandable string (`REG_EXPAND_SZ`) [pnpm/pnpm#5283](https://github.com/pnpm/pnpm/issues/5283).
