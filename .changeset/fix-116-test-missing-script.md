---
"pacquet": patch
---

`pnpm test`, `pnpm t`, and `pnpm tst` in a single-package project now fail with `ERR_PNPM_NO_SCRIPT` when the `test` script is missing unless `--if-present` is passed [#18073](https://github.com/pnpm/pnpm/pull/18073).
