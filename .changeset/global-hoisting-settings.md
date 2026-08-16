---
"@pnpm/config.reader": patch
"@pnpm/config.commands": patch
"pnpm": patch
"pacquet": patch
---

Hoisting settings can now be configured in the global `config.yaml` and via `pnpm config set <key> <val> --global` [#13858](https://github.com/pnpm/pnpm/issues/13858).
