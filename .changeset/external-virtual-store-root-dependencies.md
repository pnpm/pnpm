---
"@pnpm/installing.linking.hoist": patch
"pnpm": patch
"pacquet": patch
---

Packages in an external `virtualStoreDir` can resolve the project's direct dependencies selected by `hoistPattern`. Run `pnpm install --force` to repair an existing installation [#5652](https://github.com/pnpm/pnpm/issues/5652).
