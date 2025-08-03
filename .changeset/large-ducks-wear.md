---
"@pnpm/plugin-commands-installation": patch
"@pnpm/workspace.manifest-writer": patch
---

Add `dedupeCatalog` configuration. When its value is set to true, installing dependencies will remove unused catalog dependencies.
