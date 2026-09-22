---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm pack` now includes exactly one `package.json` in the archive when the project uses an alternative manifest format. The manifest is included even when `.npmignore` or `files` excludes the source file.
