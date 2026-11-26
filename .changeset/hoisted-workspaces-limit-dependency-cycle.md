---
"pnpm": patch
---

Fixed `pnpm install` crashing with `nodeLinker: hoisted` and `hoistingLimits: workspaces` when a project depends on a package that is part of a dependency cycle, such as `eslint` or `browserslist`. The install now completes.
