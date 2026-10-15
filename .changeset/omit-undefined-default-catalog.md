---
"@pnpm/catalogs.config": patch
"pnpm": patch
---

The catalogs read from `pnpm-workspace.yaml` no longer include a `default` entry when the workspace declares no default catalog.
