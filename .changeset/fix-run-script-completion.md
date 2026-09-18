---
"pacquet": patch
"@pnpm/cli.commands": patch
"pnpm": patch
---

Fixed shell completion of package scripts for `pnpm run` and `pnpm run-script` [pnpm/pnpm#15034](https://github.com/pnpm/pnpm/issues/15034).

Bash completion now preserves script names containing glob characters in pnpm v11 and v12.
