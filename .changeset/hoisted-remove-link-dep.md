---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

Removing a `link:` dependency now also removes its symlink from `node_modules` when `nodeLinker` is `hoisted` [#16884](https://github.com/pnpm/pnpm/issues/16884).
