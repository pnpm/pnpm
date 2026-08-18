---
"pacquet": patch
---

`pnpm --filter . deploy` deploys the project in the current directory instead of the projects nested under it, so deploying the workspace root now copies the root project and installs its workspace dependencies [#13758](https://github.com/pnpm/pnpm/issues/13758). `pnpm deploy --legacy` no longer rewrites the source workspace's `pnpm-lock.yaml`.
