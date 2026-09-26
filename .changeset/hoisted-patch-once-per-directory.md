---
"pacquet": patch
---

`pnpm install` with `nodeLinker: hoisted` now applies a patch once to each copy of a patched dependency in a workspace. Before, a copy that several workspace projects shared could receive the patch twice and end up with the patched content duplicated [#7565](https://github.com/pnpm/pnpm/issues/7565).
