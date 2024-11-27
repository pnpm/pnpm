---
"@pnpm/plugin-commands-setup": patch
"pnpm": patch
---

`pnpm setup` should remove the CLI from the target location before moving the new binary [#8173](https://github.com/pnpm/pnpm/issues/8173).
