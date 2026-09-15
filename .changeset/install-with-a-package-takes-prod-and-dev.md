---
"pacquet": patch
---

`pnpm install <pkg>` now accepts `--prod` and `--dev`, including the `--prod=false` spelling. Those flags used to abort the command with an argument parsing error [#14868](https://github.com/pnpm/pnpm/issues/14868).
