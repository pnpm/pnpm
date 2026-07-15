---
"@pnpm/fs.packlist": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed `pnpm pack` applying workspace-root ignore rules when a workspace package has its own `.npmignore` file.
