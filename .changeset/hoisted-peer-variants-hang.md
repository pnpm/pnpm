---
"pacquet": patch
---

`pnpm install` with `nodeLinker: hoisted` no longer hangs or runs out of memory in large workspaces whose packages are installed with many different sets of peer dependencies.
