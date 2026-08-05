---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm self-update` now checks that the version it installed can run before making it the active pnpm. A release that installs but cannot execute is discarded with an error instead of replacing a working installation.
