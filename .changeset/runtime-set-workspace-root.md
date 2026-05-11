---
"@pnpm/engine.runtime.commands": patch
"pnpm": patch
---

`pnpm runtime set <name> <version>` no longer fails in the root of a multi-package workspace with the `ADDING_TO_ROOT` error. Installing the workspace root is a valid target for a runtime, so the command now bypasses that safety check.
