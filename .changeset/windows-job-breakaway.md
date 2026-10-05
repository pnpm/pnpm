---
"pacquet": patch
---

On Windows, a process started by `pnpm run` or `pnpm exec` can again start a child with `CREATE_BREAKAWAY_FROM_JOB`. That child keeps running after pnpm exits, even if the command fails [#16628](https://github.com/pnpm/pnpm/issues/16628).
