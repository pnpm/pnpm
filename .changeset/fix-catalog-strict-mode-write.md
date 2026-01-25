---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fixed a bug where `catalogMode: strict` would write the literal string `"catalog:"` to `pnpm-workspace.yaml` instead of the resolved version specifier when re-adding an existing catalog dependency [#10176](https://github.com/pnpm/pnpm/issues/10176).
