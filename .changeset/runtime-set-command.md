---
"@pnpm/engine.runtime.commands": patch
"pnpm": minor
---

Added a new command `pnpm runtime set <runtime name> <runtime version spec> [-g]` for installing runtimes. Deprecated `pnpm env use` in favor of the new command.
