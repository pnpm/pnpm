---
"pacquet": patch
---

`pnpm run` and `pnpm exec` now install a selected project whose dependencies changed when a filtered install for another project ran in between. Before, the script ran without the new dependencies, and `verifyDepsBeforeRun: error` reported nothing [#17011](https://github.com/pnpm/pnpm/issues/17011).
