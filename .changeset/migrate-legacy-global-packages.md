---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm setup` reinstalls global packages recorded by the previous global directory into the current one. Their commands are linked in the pnpm home `bin` directory, and `pnpm list -g` lists them [#11528](https://github.com/pnpm/pnpm/issues/11528).
