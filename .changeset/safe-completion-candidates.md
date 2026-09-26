---
"@pnpm/cli.commands": patch
"pnpm": patch
"pacquet": patch
---

Shell completion now omits candidates containing control or invisible formatting characters. Package and script names can no longer inject extra completion records or terminal escape sequences.
