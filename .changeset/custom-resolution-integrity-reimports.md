---
"pacquet": patch
---

`pnpm install` now re-fetches a package from a custom resolver when the `integrity` of its resolution changes. It used to update the lockfile but keep the old files in `node_modules`. Installs with `enableGlobalVirtualStore` still keep the old files [#15670](https://github.com/pnpm/pnpm/issues/15670).
