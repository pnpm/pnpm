---
"pacquet": minor
---

Added the `-w` / `--workspace-root` option, which runs the command on the root workspace project from any directory inside the workspace — so `pnpm add -D pkg-a pkg-b -w` from a workspace package saves the dependencies to the workspace root's `package.json` [#13031](https://github.com/pnpm/pnpm/issues/13031). Combining it with `--global` fails with `ERR_PNPM_OPTIONS_CONFLICT`, and using it outside a workspace fails with `ERR_PNPM_NOT_IN_WORKSPACE`.
