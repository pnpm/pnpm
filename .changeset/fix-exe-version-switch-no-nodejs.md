---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Fixed version switching via `packageManager` field failing when pnpm is installed as a standalone executable in environments without a system Node.js [#10687](https://github.com/pnpm/pnpm/issues/10687).
