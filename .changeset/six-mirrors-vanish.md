---
"pacquet": minor
---

`--allow-build` now accepts a comma-separated list of packages, so `pnpm add foo bar --allow-build=foo,bar` is the same as passing `--allow-build=foo --allow-build=bar` [#9560](https://github.com/pnpm/pnpm/pull/9560).
