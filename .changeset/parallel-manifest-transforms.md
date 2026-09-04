---
"pacquet": patch
---

Sped up dependency resolution in large workspaces. pnpm now prepares project manifests for resolution in parallel and renders workspace link paths in a single pass [#14352](https://github.com/pnpm/pnpm/issues/14352).
