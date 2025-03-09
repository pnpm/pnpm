---
"@pnpm/plugin-commands-patching": patch
"pnpm": patch
---

`pnpm patch-commit` will now use the same filesystem as the store directory or the `node_modules` directory to compare and create patch files.
