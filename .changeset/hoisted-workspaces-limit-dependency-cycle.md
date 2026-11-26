---
"pnpm": patch
---

`pnpm install` no longer crashes with `nodeLinker: hoisted` and `hoistingLimits: projects` when a project depends on a package in a dependency cycle, such as `eslint` or `browserslist`.
