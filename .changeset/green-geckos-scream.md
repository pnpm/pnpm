---
"@pnpm/config": patch
"pnpm": patch
---

`NODE_ENV=production pnpm install --dev` should only install dev deps [#4745](https://github.com/pnpm/pnpm/pull/4745).
