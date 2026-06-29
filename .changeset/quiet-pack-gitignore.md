---
"@pnpm/fs.packlist": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed `pnpm pack` in workspace packages so it applies `.gitignore` rules from the workspace root when no package-local `.npmignore` is present.
