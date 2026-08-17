---
"@pnpm/config.reader": minor
"pnpm": minor
---

A project's `pnpm-workspace.yaml` can no longer choose where pnpm keeps its credentials, its own installation, or the registry it downloads its next version from. One of those settings is `configDir`, which decided where `pnpm login` writes the granted token. `bin`, `dir`, `globalBinDir`, `globalDir`, `npmrcAuthFile`, `pnpmHomeDir`, `stateDir`, `userconfig` and `workspaceDir` are ignored there now too, and pnpm warns about the ones it finds. `cacheDir` and `storeDir` are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).
