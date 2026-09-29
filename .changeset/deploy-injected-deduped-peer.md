---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` no longer fails with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` in a workspace with `injectWorkspacePackages: true` when a workspace package lists its peer dependency as a dev dependency too [#16375](https://github.com/pnpm/pnpm/issues/16375).
