---
"pacquet": patch
---

`pnpm deploy` no longer fails with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` when a workspace package declares a `workspace:` peer dependency and other packages in the deployed graph depend on registry versions of that peer. The peer now resolves to the workspace package [#16807](https://github.com/pnpm/pnpm/issues/16807).
