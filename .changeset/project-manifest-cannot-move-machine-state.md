---
"@pnpm/config.commands": patch
"@pnpm/config.reader": minor
"pnpm": minor
---

A project's `pnpm-workspace.yaml` can no longer choose where pnpm keeps its credentials and its own installation — among those settings `configDir`, which decided where `pnpm login` writes the granted token. `bin`, `dir`, `globalBinDir`, `globalDir`, `npmrcAuthFile`, `pnpmHomeDir`, `stateDir`, `userconfig` and `workspaceDir` are ignored there now too, and pnpm warns about the ones it finds. Set them with `pnpm config set --global` where that is supported; `pnpm config set` says which. `cacheDir` and `storeDir` are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).
