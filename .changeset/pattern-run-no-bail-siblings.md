---
"pacquet": patch
---

`pnpm run "/pattern/" --no-bail` now lets every matched script finish after one of them fails. The command then exits with `ERR_PNPM_RUN_FAILED` and lists the scripts that failed, as pnpm 11 does [#14718](https://github.com/pnpm/pnpm/issues/14718).
