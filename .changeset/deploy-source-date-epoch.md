---
"pacquet": minor
---

`pnpm deploy` and `pnpm install` now honor the `SOURCE_DATE_EPOCH` environment variable. The time it sets replaces the current time in `node_modules/.modules.yaml` and `node_modules/.pnpm-workspace-state-v1.json`, so deploying the same project twice produces identical files [pnpm/pnpm#17171](https://github.com/pnpm/pnpm/issues/17171).
