---
"pacquet": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` no longer fails with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` when a workspace package declares a `workspace:` peer dependency that it also lists as a dev dependency, and other packages in the deployed graph depend on registry versions of that peer. The peer resolves to the linked workspace package when the deployed graph includes it [#16807](https://github.com/pnpm/pnpm/issues/16807).
