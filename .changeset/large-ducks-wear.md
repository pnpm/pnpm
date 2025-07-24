---
"@pnpm/plugin-commands-installation": patch
"@pnpm/workspace.manifest-writer": patch
---

When `catalogMode` is `strict`, the corresponding catalog configuration is deleted when the dependent package is removed.
