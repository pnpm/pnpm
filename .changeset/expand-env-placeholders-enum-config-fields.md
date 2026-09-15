---
"pacquet": patch
---

Fixed configuration parsing in `pnpm-workspace.yaml` and `.npmrc` failing when enum-valued settings such as `nodeLinker` contain environment variable placeholders with fallback syntax [#14914](https://github.com/pnpm/pnpm/issues/14914).
