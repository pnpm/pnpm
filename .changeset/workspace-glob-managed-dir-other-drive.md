---
"pacquet": patch
---

Fixed `pnpm install` failing with `ERR_PNPM_WORKSPACE_INVALID_GLOB` on Windows for a wildcard pattern such as `plugins/*/*` in `pnpm-workspace.yaml` when the workspace is on a different drive than the pnpm cache or state directory [#16239](https://github.com/pnpm/pnpm/issues/16239).
