---
"pacquet": patch
---

The `pnpm install --help` descriptions of `--prod` and `--dev` no longer claim that the flags take precedence over `NODE_ENV`. pnpm does not read `NODE_ENV` when selecting which dependency groups to install [#14445](https://github.com/pnpm/pnpm/issues/14445).
