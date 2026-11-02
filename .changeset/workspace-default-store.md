---
"pacquet": patch
---

Commands run from a workspace package now use the same default store as the workspace root. Before, `pnpm add` in a package could fail with `ERR_PNPM_UNEXPECTED_STORE` when the workspace root was its own mount point and the home store was on another volume, as in a sandbox or a container.

A workspace installed with the old behavior may need one `pnpm install --force` from the workspace root.
