---
"pacquet": minor
---

Added the `preferredManifestFormat` setting to `pnpm-workspace.yaml`. It picks which manifest a workspace project uses when its directory has more than one of `package.json`, `package.json5`, and `package.yaml`. The values are `json` (the default), `json5`, and `yaml`. pnpm reads and writes the preferred file, and falls back to the usual order when that file is missing. This lets a project keep a stub `package.json` for other tools while its real manifest is a commented `package.json5` [#3027](https://github.com/pnpm/pnpm/issues/3027) [#5541](https://github.com/pnpm/pnpm/issues/5541).
