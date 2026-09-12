---
"pacquet": patch
---

`pnpm install` respects negated `packages` patterns when discovering Cargo and Python projects. Excluded independent workspaces are not parsed or modified.
