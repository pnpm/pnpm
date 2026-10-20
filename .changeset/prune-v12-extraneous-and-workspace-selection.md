---
"pacquet": patch
---

`pnpm prune` now removes extraneous direct packages from `node_modules` when `package.json` and `pnpm-lock.yaml` are already in sync. `pnpm prune` also honors workspace project selection (`--filter`, `-r`) and `sharedWorkspaceLockfile: false` in workspaces.
