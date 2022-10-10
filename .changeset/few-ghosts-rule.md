---
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

`pnpm link <pkg> --global` should work when a custom target directory is specified with the `--dir` CLI option [#5473](https://github.com/pnpm/pnpm/pull/5473).
