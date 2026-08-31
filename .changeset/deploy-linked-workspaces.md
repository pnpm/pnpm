---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now supports workspaces that keep `injectWorkspacePackages` disabled. Instead of failing with `ERR_PNPM_DEPLOY_NONINJECTED_WORKSPACE`, the deploy lockfile rewrites the linked workspace dependencies to `file:` dependencies that are materialized inside the deploy directory [#9386](https://github.com/pnpm/pnpm/issues/9386).
