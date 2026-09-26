---
"@pnpm/global.commands": patch
"@pnpm/global.packages": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` mutating global bins or install directories after only partially reading an installed package group. If any declared package manifest is missing, malformed, or unreadable, pnpm now fails before activation or removal and leaves the existing global installation intact [pnpm/pnpm#13796](https://github.com/pnpm/pnpm/issues/13796).
