---
"pacquet": patch
---

`pnpm approve-builds -g` is accepted again, reporting that the command is not supported with global packages rather than failing with `unexpected argument '-g' found`. `approve-builds` was the only command that declared `--global` without its `-g` short form [#13310](https://github.com/pnpm/pnpm/issues/13310).
