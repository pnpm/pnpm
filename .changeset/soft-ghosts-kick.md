---
"pnpm": minor
---

A new setting added for symlinking [injected dependencies](https://pnpm.io/package_json#dependenciesmetainjected) from the workspace, if their dependencies use the same peer dependencies as the dependent package. The setting is called `dedupe-injected-deps` [#7416](https://github.com/pnpm/pnpm/pull/7416).
