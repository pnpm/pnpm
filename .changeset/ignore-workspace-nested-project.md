---
"pacquet": patch
---

`pnpm install` and `pnpm update` now honor `--ignore-workspace` in a project nested under a workspace root but excluded from its `packages` patterns. They previously installed every project of the surrounding workspace. `pnpm-lock.yaml` was written at the workspace root. A build script belonging to that root failed the command with `ERR_PNPM_IGNORED_BUILDS`. The flag now also covers the `packageManager` check that runs before every command, which failed with `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS` on an unrecognized setting in the ignored file [#14809](https://github.com/pnpm/pnpm/issues/14809).
