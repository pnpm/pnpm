---
"@pnpm/parse-cli-args": patch
"pnpm": patch
---

Fix a regression in which `pnpm dlx pkg --help` doesn't pass `--help` to `pkg` [#9823](https://github.com/pnpm/pnpm/issues/9823).
