---
"pacquet": patch
---

`pnpm install --prod` and `pnpm install --dev` now record every dependency group in `pnpm-lock.yaml`. `node_modules` still holds only the groups the filter selects. They used to write the filter into the lockfile, so a later `pnpm install --frozen-lockfile` rejected it. `pnpm prune --prod`, `pnpm prune --dev`, and `pnpm prune --no-optional` behave the same way [#14912](https://github.com/pnpm/pnpm/issues/14912).
