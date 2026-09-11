---
"pacquet": patch
---

`pnpm install` and `pnpm update` now honor `--ignore-workspace` in a project nested under a workspace root but excluded from its `packages` patterns [#14809](https://github.com/pnpm/pnpm/issues/14809). The whole surrounding workspace was installed instead. `pnpm-lock.yaml` was written at the workspace root, every sibling project was installed, and build scripts belonging to the workspace root failed the command with `ERR_PNPM_IGNORED_BUILDS`.
