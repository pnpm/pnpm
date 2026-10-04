---
"pacquet": minor
---

`pnpm` now loads dotenv-style files named by a repeatable global `--env-file <path>` flag before running, so `pnpm --env-file .env run build` no longer needs a `dotenv-cli` or `node --env-file` wrapper. The first file naming a variable wins, variables already in the environment are never overridden, and a missing or malformed file fails the command. Loaded variables are visible to config resolution (including `${VAR}` tokens in `.npmrc` files), to lifecycle scripts, and to any pnpm the command dispatches to. Related to [#7111](https://github.com/pnpm/pnpm/issues/7111).
