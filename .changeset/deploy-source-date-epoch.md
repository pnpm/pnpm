---
"pacquet": minor
---

`pnpm deploy` and `pnpm install` now use the `SOURCE_DATE_EPOCH` environment variable as an upper bound for timestamps in `node_modules/.modules.yaml` and `node_modules/.pnpm-workspace-state-v1.json`. With an equivalently prepared deploy target and an epoch no later than the timestamps that would otherwise be recorded, deploying the same project twice produces identical files [pnpm/pnpm#17171](https://github.com/pnpm/pnpm/issues/17171).
