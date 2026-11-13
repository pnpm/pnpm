---
"pacquet": patch
---

`pnpm config set --global --json tools` now writes `tools` to the global `config.yaml` and validates its structure before saving. `blockExoticSubdeps` and `excludeLinksFromLockfile` set in the global `config.yaml` now take effect during installs and appear in `pnpm config get --global`.
