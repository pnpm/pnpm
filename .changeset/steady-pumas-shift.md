---
"pnpm": major
---

Build approvals from `pnpm approve-builds` and `pnpm add --allow-build` now use `permissions`. Existing `allowBuilds` decisions still work. pnpm moves an entry when a new decision is saved for that package. pnpm warns when `allowBuilds` contains a decision [pnpm/tasks#118](https://github.com/pnpm/tasks/issues/118).
