---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

Skip Git checks when publishing explicit directory or tarball arguments. Publishing the current project without arguments or recursively publishing a workspace still runs Git checks.
