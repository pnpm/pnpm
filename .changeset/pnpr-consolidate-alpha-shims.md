---
"@pnpm/pnpr": patch
---

Removed `pnpr upgrade-storage` and `pnpr config-check`. Removed the `--packument-ttl-secs`, `--no-check-hashes`, `--min-age-secs`, and `--max-age-secs` CLI flag aliases and `PNPR_PACKUMENT_TTL_SECS`. Standardized YAML configuration keys on `camelCase`, including `maxUsers`, `tokenEnv`, `maxAge`, `syncInterval`, and `backend.postgres`.
