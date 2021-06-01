---
"@pnpm/parse-cli-args": minor
---

A new option added: `escapeArgs`. `escapeArgs` is an array of arguments that stop arguments parsing.
By default, everything after `--` is not parsed as key-value. This option allows to add new keywords to stop parsing.
