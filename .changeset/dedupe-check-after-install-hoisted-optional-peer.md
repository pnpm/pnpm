---
"pacquet": patch
---

`pnpm dedupe --check` no longer fails right after `pnpm install` when a project's optional peer is satisfied by a package another workspace project installs. `pnpm dedupe` now picks the same versions for that package's dependencies as `pnpm install` [#16447](https://github.com/pnpm/pnpm/issues/16447).
