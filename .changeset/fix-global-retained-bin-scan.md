---
"@pnpm/global.commands": patch
"@pnpm/bins.linker": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add -g` and `pnpm update -g` now ignore incomplete unrelated global package groups when every command from the replaced group is retained. Operations that could remove a global command still require complete ownership information.
