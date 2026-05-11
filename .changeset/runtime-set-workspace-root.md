---
"@pnpm/engine.runtime.commands": patch
"pnpm": patch
---

`pnpm runtime set <name> <version>` no longer fails in the root of a multi-package workspace with the `ADDING_TO_ROOT` error. Installing a runtime is workspace-wide configuration, so the command now implicitly targets the workspace root.
