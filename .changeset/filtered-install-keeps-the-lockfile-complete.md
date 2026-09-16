---
"pacquet": patch
---

`pnpm install --prod` and `pnpm install --dev` now record every dependency group in `pnpm-lock.yaml`, so a later `pnpm install --frozen-lockfile` succeeds [#14912](https://github.com/pnpm/pnpm/issues/14912). They used to write the filter into the lockfile and drop the groups they did not install. `node_modules` still holds only the selected groups. `pnpm prune --prod`, `pnpm prune --dev`, and `pnpm prune --no-optional` follow the same rule.
