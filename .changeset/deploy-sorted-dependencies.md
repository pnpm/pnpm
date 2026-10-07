---
"pacquet": patch
---

`pnpm deploy` now writes the dependencies in the deployed `package.json` and the `allowBuilds` entries in the deployed `pnpm-workspace.yaml` sorted by name, so repeated deploys of the same lockfile produce identical files [#16687](https://github.com/pnpm/pnpm/issues/16687).
