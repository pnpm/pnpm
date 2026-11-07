---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

With `nodeLinker: hoisted`, `pnpm install --filter` in a sparse checkout now drops previously installed workspace projects that are missing from disk from the hoisted `node_modules` tree. It no longer recreates `node_modules` inside a missing project's directory or keeps packages that only the missing project used.
