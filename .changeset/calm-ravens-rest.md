---
"@pnpm/plugin-commands-setup": patch
"pnpm": patch
---

`pnpm setup` should update the config of the current shell, not the preferred shell.
