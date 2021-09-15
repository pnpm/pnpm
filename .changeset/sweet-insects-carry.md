---
"@pnpm/filter-workspace-packages": patch
"@pnpm/git-fetcher": patch
"@pnpm/git-resolver": patch
"@pnpm/plugin-commands-publishing": patch
"@pnpm/plugin-commands-script-runners": patch
"@pnpm/plugin-commands-setup": patch
---

Use safe-execa instead of execa to prevent binary planting attacks on Windows.
