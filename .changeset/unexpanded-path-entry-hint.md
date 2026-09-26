---
"@pnpm/config.reader": patch
"pacquet": patch
"pnpm": patch
---

On Windows, if the global bin directory is not in `PATH` and a `PATH` entry still contains an unexpanded variable such as `%PNPM_HOME%`, the error now names that entry. A variable referenced from the user `Path` must be set to a full path and stored as a plain string (`REG_SZ`) for the entry to expand [pnpm/pnpm#5283](https://github.com/pnpm/pnpm/issues/5283).
