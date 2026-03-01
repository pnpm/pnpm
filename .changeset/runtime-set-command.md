---
"@pnpm/plugin-commands-env": patch
"@pnpm/runtime.commands": minor
"pnpm": minor
---

Added a new command `pnpm runtime set <runtime name> <runtime version spec> [-g]` for installing runtimes. Deprecated `pnpm env use` in favor of the new command.
