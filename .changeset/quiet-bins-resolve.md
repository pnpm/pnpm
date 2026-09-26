---
"@pnpm/installing.context": patch
"pacquet": patch
"pnpm": patch
---

`node_modules/.bin` scripts no longer resolve undeclared dependencies from pnpm's private hoist directory [pnpm/pnpm#11351](https://github.com/pnpm/pnpm/issues/11351).
