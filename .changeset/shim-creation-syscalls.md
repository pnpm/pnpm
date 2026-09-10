---
"pacquet": patch
---

pnpm does less filesystem work when an install creates the command shims in `node_modules/.bin`. A warm install of a 76 project workspace makes about 1,500 fewer filesystem calls [#14540](https://github.com/pnpm/pnpm/issues/14540).
