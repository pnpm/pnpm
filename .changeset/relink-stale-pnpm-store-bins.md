---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

pnpm now relinks the bins of a pnpm version it installed earlier for `packageManager` or `pnpm with`. A pnpm 12 version installed by a pnpm release older than 11.28.4 could keep a launcher that ran `node` on the native binary, so every command failed with `SyntaxError: Invalid or unexpected token` [#16646](https://github.com/pnpm/pnpm/issues/16646).
