---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

With `nodeLinker: hoisted`, `pnpm install` now removes the commands of a package it removes from `node_modules/.bin`. A nested copy that was deduped into the root `node_modules` used to leave behind a command pointing at a missing package [#7568](https://github.com/pnpm/pnpm/issues/7568).
