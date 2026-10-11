---
"pacquet": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy` without `injectWorkspacePackages` now binds a peer dependency of a linked workspace package to the version its parent provides, as an injected install does. Previously the deploy failed with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` whenever other packages in the deployed graph depended on different versions of that peer [#16807](https://github.com/pnpm/pnpm/issues/16807).
