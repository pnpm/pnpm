---
"pacquet": patch
---

`pnpm deploy` now preserves `lockfile.format` and `lockfileIncludeTarballUrl` in the deployed `pnpm-workspace.yaml`. Previously, commands such as `pnpm dedupe --check` in a deployed project using `lockfile.format: v13` treated the lockfile as `v9` and reported changes.
