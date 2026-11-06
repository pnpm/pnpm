---
"@pnpm/installing.linking.modules-cleaner": patch
"pnpm": patch
"pacquet": patch
---

With `dedupeDirectDeps`, `pnpm install` reports a project dependency that the workspace root provides as removed only once, when it removes the project's link [pnpm/tasks#293](https://github.com/pnpm/tasks/issues/293).
