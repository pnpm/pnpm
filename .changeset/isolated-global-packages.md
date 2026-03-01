---
"@pnpm/global.packages": minor
"@pnpm/global.commands": minor
"@pnpm/plugin-commands-installation": major
"@pnpm/plugin-commands-listing": minor
"@pnpm/plugin-commands-script-runners": minor
"pnpm": major
---

Isolated global packages. Each globally installed package (or group of packages installed together) now gets its own isolated installation directory with its own `package.json`, `node_modules/`, and lockfile. This prevents global packages from interfering with each other through peer dependency conflicts, hoisting changes, or version resolution shifts.

Key changes:
- `pnpm add -g <pkg>` creates an isolated installation in `{pnpmHomeDir}/global/v11/{hash}/`
- `pnpm remove -g <pkg>` removes the entire installation group containing the package
- `pnpm update -g [pkg]` re-installs packages in new isolated directories
- `pnpm list -g` scans isolated directories to show all installed global packages
- `pnpm install -g` (no args) is no longer supported; use `pnpm add -g <pkg>` instead
