---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

`pnpm install --node-linker=hoisted` no longer downloads every optional dependency it reports as skipped when `node_modules` already exists. Those downloads also continued after pnpm printed `Done` [#14139](https://github.com/pnpm/pnpm/issues/14139).
