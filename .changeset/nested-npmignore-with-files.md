---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now honor a `.npmignore` or `.gitignore` inside a subdirectory when `package.json` has a `files` field. The package root's ignore files and those inherited from the workspace are still skipped [#16909](https://github.com/pnpm/pnpm/issues/16909).
