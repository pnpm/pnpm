---
"pacquet": patch
---

`pnpm approve-builds`, `pnpm approve`, and `pnpm add --allow-build` now refuse to edit a package's `permissions` entry written as an inline mapping that spans several lines. Before, they added a second key for the package to `pnpm-workspace.yaml`.
