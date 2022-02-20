---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

When NODE_ENV is set to "production", inform the user that devDependencies will not be installed.
