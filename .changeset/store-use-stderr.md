---
"pnpm": patch
---

Reporter output (warnings, progress) for `pnpm store` and `pnpm config` subcommands now goes to stderr instead of stdout. This fixes scripts that capture their stdout (e.g. `PNPM_STORE=$(pnpm store path)`, `pnpm config list --json | jq`) from getting warnings mixed into the result.
