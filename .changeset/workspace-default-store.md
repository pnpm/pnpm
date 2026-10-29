---
"pacquet": patch
---

Use the workspace root when choosing the default store location. Running `pnpm add` from a workspace package could fail with `ERR_PNPM_UNEXPECTED_STORE` when a sandbox restricted access to the home store.
