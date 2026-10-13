---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now honor a `.npmignore` or `.gitignore` inside a subdirectory when `package.json` has a `files` field. Only the root ignore files are skipped in that case, as with npm [#16909](https://github.com/pnpm/pnpm/issues/16909).
