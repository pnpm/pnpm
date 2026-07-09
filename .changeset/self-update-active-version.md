---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm self-update <version>` now installs the requested pnpm version when it matches the currently running version but is missing from the global self-update directory.
