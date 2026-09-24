---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` with `--filter` now installs only the dependencies of the selected projects when using `nodeLinker: hoisted` [#8882](https://github.com/pnpm/pnpm/issues/8882).
