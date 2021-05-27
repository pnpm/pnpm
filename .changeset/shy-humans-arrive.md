---
"@pnpm/parse-cli-args": patch
---

The `--help` option should not convert the command to `help` if the command is unknown. So `pnpm eslint -h` is not parsed as `pnpm help eslint`.
