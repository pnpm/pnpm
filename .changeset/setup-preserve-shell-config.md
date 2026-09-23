---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm setup` preserves preceding comments and shell configuration lines (such as aliases or NVM setup) when updating shell startup files.
