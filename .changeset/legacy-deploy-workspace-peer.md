---
"pacquet": patch
---

`pnpm deploy --legacy` no longer fails with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` when a workspace dependency of the deployed project declares a `workspace:` peer dependency that the project does not depend on itself. pnpm now copies that peer from the workspace into the deploy directory.

An auto-installed peer declared with `workspace:` now always resolves to the workspace project. Previously, `workspace:*` and `workspace:^1.0.0` peers could be installed from the registry [#16806](https://github.com/pnpm/pnpm/issues/16806).
