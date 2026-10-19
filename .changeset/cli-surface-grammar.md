---
"@pnpm/pnpr": patch
---

Added the `pnpr serve` subcommand. Bare `pnpr` remains supported as an alias. Server flags such as `--listen` and `--packument-ttl` are now scoped to `serve` and are no longer accepted on maintenance commands.
