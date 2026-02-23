---
"pnpm": patch
---

Fixes #10594, catalogs not being read from the workspace when using the `catalog:` protocol with the `pnpm dlx` / `pnpx` command, resulting in a catalog entry not found error.
