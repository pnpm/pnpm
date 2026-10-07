---
"pacquet": patch
---

`pnpm deploy` now writes the dependencies in the deployed `package.json` sorted by name. It also sorts the `allowBuilds` entries in the deployed `pnpm-workspace.yaml`. Repeated deploys of the same lockfile now produce identical files [#16687](https://github.com/pnpm/pnpm/issues/16687).
