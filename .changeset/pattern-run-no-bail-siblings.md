---
"pacquet": patch
---

`pnpm run "/pattern/" --no-bail` now lets every matched script finish after one of them fails. The command then exits with `ERR_PNPM_RUN_FAILED`. Its message lists the scripts that failed, in the order they were selected [#14718](https://github.com/pnpm/pnpm/issues/14718).
