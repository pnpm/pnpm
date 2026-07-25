---
"pacquet": patch
---

Fixed `--workspace-root` (`-w`) selecting the current workspace when `--dir` pointed at a nonexistent directory outside it (for example `pnpm --dir ../../elsewhere add -w foo`). The command now fails with `ERR_PNPM_NOT_IN_WORKSPACE`, matching pnpm. A nonexistent `--dir` inside the workspace still resolves to the workspace root as before.
