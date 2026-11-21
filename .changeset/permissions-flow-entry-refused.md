---
"pacquet": patch
---

`pnpm approve-builds`, `pnpm approve`, and `pnpm add --allow-build` now refuse to edit a package's `permissions` entry written as an inline mapping such as `sharp: { build: true }`. Before, they added a second `sharp` key to `pnpm-workspace.yaml`.
