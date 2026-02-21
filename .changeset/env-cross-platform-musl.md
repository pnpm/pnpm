---
"@pnpm/node.fetcher": minor
"@pnpm/plugin-commands-env": minor
"pnpm": minor
---

On systems using the musl C library (e.g. Alpine Linux), `pnpm env use` now automatically downloads the musl variant of Node.js from [unofficial-builds.nodejs.org](https://unofficial-builds.nodejs.org).

`pnpm env use` now installs Node.js via pnpm's own package install machinery. Node.js is stored in the global virtual store under `{pnpmHomeDir}/env/`, which means `pnpm store prune` will clean up unused Node.js versions automatically.

The `pnpm env add` and `pnpm env remove` subcommands have been removed. Use `pnpm env use` to install and activate a Node.js version. `pnpm env list` now only lists remote Node.js versions (the `--remote` flag is no longer required).
