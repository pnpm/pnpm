---
"@pnpm/plugin-commands-installation": patch
---

Do not run installation in the global package, when linking a dependency using `pnpm link -g <pkg name>`.
