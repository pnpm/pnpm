---
"pacquet": patch
---

`pnpm run` and `pnpm exec` now print `No projects matched the filters in "<workspace>"` when `--filter` selects no project, as pnpm v11 does. Before, they exited silently [pnpm/pnpm#8408](https://github.com/pnpm/pnpm/issues/8408).
