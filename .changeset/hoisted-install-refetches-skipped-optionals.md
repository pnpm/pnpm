---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

`pnpm install --node-linker=hoisted` no longer downloads every optional dependency it reports as skipped when `node_modules` already exists. The downloads also kept running after pnpm printed `Done`, since nothing waited on them [#14139](https://github.com/pnpm/pnpm/issues/14139).
