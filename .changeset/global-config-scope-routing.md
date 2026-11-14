---
"pacquet": patch
---

`pnpm config set --global --json tools` now writes `tools` to the global `config.yaml` and validates its structure before saving. `pnpm config set --global` also accepts `pm-on-fail` and `runtime-on-fail`. `blockExoticSubdeps` and `excludeLinksFromLockfile` set in the global `config.yaml` now take effect during installs and appear in `pnpm config get --global`. Workspace-only settings placed in the global `config.yaml` are ignored with a warning, matching `pnpm config set --global`.
