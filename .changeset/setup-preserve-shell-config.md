---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm setup` no longer deletes aliases and other lines that sit between a `# pnpm` comment and the pnpm block in a shell startup file [#7067](https://github.com/pnpm/pnpm/issues/7067).
