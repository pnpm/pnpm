---
"@pnpm/pkg-manifest.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r pkg get` now reports every selected project when several share a package name. Projects with the same name are keyed by their directory relative to the workspace root. Before, only one of them appeared in the output.
