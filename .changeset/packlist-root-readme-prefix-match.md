---
"@pnpm/fs.packlist": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pack` only includes README and LICENSE files matching exact base names with optional extensions at the package root, rather than any file prefixed with "readme" or "license".
