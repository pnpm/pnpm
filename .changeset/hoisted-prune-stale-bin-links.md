---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

With `nodeLinker: hoisted`, `pnpm install` now removes the commands of the packages it removes from `node_modules/.bin`, such as a nested copy deduped into the root `node_modules` [#7568](https://github.com/pnpm/pnpm/issues/7568).
