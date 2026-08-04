---
"@pnpm/config.commands": patch
"@pnpm/config.reader": minor
"pnpm": minor
---

A project's `pnpm-workspace.yaml` can no longer choose where pnpm keeps its credentials and its own installation — among those settings `configDir`, which decided where `pnpm login` writes the granted token. `bin`, `dir`, `globalBinDir`, `globalDir`, `npmrcAuthFile`, `stateDir`, `userconfig` and `workspaceDir` are ignored there now too, and pnpm warns about the ones it finds. `pnpm config set` refuses to write them to a project manifest, and `pnpm config delete` still clears one a manifest already has. `cacheDir` and `storeDir` are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).
