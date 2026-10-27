---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

`pnpm pack` and `pnpm publish` now always include the binary file declared by a string `bin` field in `package.json`, even when `files` or `.npmignore` would otherwise exclude it.
