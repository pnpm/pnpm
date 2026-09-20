---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

The install summary now reports an optional dependency that a `node-linker=hoisted` install takes out of `node_modules` because this run cannot use it. The install removed the directory and said nothing.
