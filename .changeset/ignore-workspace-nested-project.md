---
"pacquet": patch
---

`pnpm install` and `pnpm update` now honor `--ignore-workspace` in a project nested under a workspace root but excluded from its `packages` patterns [#14809](https://github.com/pnpm/pnpm/issues/14809). They previously installed every project of the surrounding workspace and wrote `pnpm-lock.yaml` at the workspace root. A build script belonging to the workspace root also failed the command with `ERR_PNPM_IGNORED_BUILDS`.
