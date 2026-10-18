---
"@pnpm/installing.deps-resolver": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now records the peer dependencies of the workspace packages it copies into the deployed lockfile. Before, pnpm rejected that lockfile with `--frozen-lockfile` when a deployed workspace package declared a peer and `injectWorkspacePackages` was off [#17093](https://github.com/pnpm/pnpm/issues/17093).
